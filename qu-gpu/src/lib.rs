//! WebGPU compute kernels for Qu numerical workloads.
//!
//! GPU results are always checked against `qu-core`, which remains the semantic
//! oracle. RPGx is evaluated as a population batch to amortize dispatch and
//! transfer costs across players.

use std::sync::mpsc;

use bytemuck::{Pod, Zeroable};
use thiserror::Error;
use wgpu::util::DeviceExt;

#[derive(Debug, Error)]
pub enum GpuError {
    #[error("no compatible WebGPU adapter was found")]
    NoAdapter,
    #[error("cannot create WebGPU device: {0}")]
    Device(String),
    #[error("multisine batch has invalid dimensions")]
    Shape,
    #[error("matmul operand shapes don't agree")]
    MatmulShape,
    #[error("GPU buffer mapping failed: {0}")]
    Map(String),
    #[error("buffer of {size} bytes exceeds this device's max buffer binding size of {limit} bytes — split the workload into smaller chunks")]
    BufferTooLarge { size: u64, limit: u64 },
    #[error("a {dimension} dispatch of {count} workgroups exceeds this device's max compute workgroups per dimension ({limit}) — split the workload into smaller chunks (e.g. a tall-and-skinny matmul, many rows against a small inner/output dimension, needs its ROW count chunked)")]
    DispatchTooLarge { dimension: &'static str, count: u32, limit: u32 },
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    sample_rate_hz: f32,
    sample_count: u32,
    tone_count: u32,
    player_count: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MatmulParams {
    m: u32,
    k: u32,
    n: u32,
    _pad: u32,
}

pub struct GpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    matmul_pipeline: wgpu::ComputePipeline,
    matmul_layout: wgpu::BindGroupLayout,
    adapter_info: wgpu::AdapterInfo,
}

impl GpuContext {
    pub async fn new() -> Result<Self, GpuError> {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok_or(GpuError::NoAdapter)?;
        let adapter_info = adapter.get_info();
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("Qu compute device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_defaults(),
                },
                None,
            )
            .await
            .map_err(|error| GpuError::Device(error.to_string()))?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Qu batched multisine shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("multisine.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Qu multisine bind layout"),
            entries: &[
                storage_entry(0, true),
                storage_entry(1, true),
                storage_entry(2, true),
                storage_entry(3, false),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Qu multisine pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Qu batched multisine pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: "main",
        });

        let matmul_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Qu matmul shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("matmul.wgsl").into()),
        });
        let matmul_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Qu matmul bind layout"),
            entries: &[
                storage_entry(0, true),
                storage_entry(1, true),
                storage_entry(2, false),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let matmul_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Qu matmul pipeline layout"),
            bind_group_layouts: &[&matmul_layout],
            push_constant_ranges: &[],
        });
        let matmul_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Qu matmul pipeline"),
            layout: Some(&matmul_pipeline_layout),
            module: &matmul_shader,
            entry_point: "main",
        });

        Ok(Self {
            device,
            queue,
            pipeline,
            layout,
            matmul_pipeline,
            matmul_layout,
            adapter_info,
        })
    }

    pub fn new_blocking() -> Result<Self, GpuError> {
        pollster::block_on(Self::new())
    }

    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.adapter_info
    }

    /// Guards every storage-buffer allocation below: wgpu's own validation
    /// rejects a `create_bind_group` whose buffer exceeds
    /// `max_storage_buffer_binding_size`, but it does so via an uncaptured-
    /// error callback that panics the process by default rather than
    /// returning a `Result` — so callers must check first to keep an
    /// oversized request a normal, catchable `GpuError` instead of a crash.
    fn check_buffer_size(&self, size: u64) -> Result<(), GpuError> {
        let limit = self.device.limits().max_storage_buffer_binding_size as u64;
        if size > limit {
            return Err(GpuError::BufferTooLarge { size, limit });
        }
        Ok(())
    }

    /// Guards every `dispatch_workgroups` call below: wgpu's own validation
    /// rejects a dispatch whose workgroup count on ANY dimension exceeds
    /// `max_compute_workgroups_per_dimension` (WebGPU's mandated floor is
    /// 65535, same as the classic D3D12/Vulkan/Metal limit this maps onto),
    /// but — same story as `check_buffer_size` above — it does so via an
    /// uncaptured-error callback that panics the process by default rather
    /// than returning a `Result`. First hit by `matmul`'s own dispatch,
    /// `(n.div_ceil(16), m.div_ceil(16), 1)`: a "tall and skinny" matmul
    /// (e.g. `particle_filter`'s per-particle motion update in `qu-interp`
    /// — millions of rows against a handful of state dimensions) drives
    /// `m.div_ceil(16)` past 65535 long before either buffer hits
    /// `check_buffer_size`'s own limit, so this needs its own, separate
    /// precheck rather than being caught incidentally by that one.
    fn check_workgroup_count(&self, dimension: &'static str, count: u32) -> Result<(), GpuError> {
        let limit = self.device.limits().max_compute_workgroups_per_dimension;
        if count > limit {
            return Err(GpuError::DispatchTooLarge { dimension, count, limit });
        }
        Ok(())
    }

    /// Synthesize `player_count` waveforms. `phases` is row-major
    /// `[player, tone]`; the output is row-major `[player, sample]`.
    pub fn synthesize_batch(
        &self,
        sample_rate_hz: f32,
        sample_count: usize,
        frequencies_hz: &[f32],
        amplitudes: &[f32],
        phases: &[f32],
    ) -> Result<Vec<f32>, GpuError> {
        let tone_count = frequencies_hz.len();
        if sample_rate_hz <= 0.0
            || sample_count == 0
            || tone_count == 0
            || amplitudes.len() != tone_count
            || phases.len() % tone_count != 0
        {
            return Err(GpuError::Shape);
        }
        let player_count = phases.len() / tone_count;
        if player_count == 0 {
            return Err(GpuError::Shape);
        }
        let output_len = player_count
            .checked_mul(sample_count)
            .ok_or(GpuError::Shape)?;
        self.check_buffer_size((output_len * std::mem::size_of::<f32>()) as u64)?;
        self.check_workgroup_count("x", (output_len as u32).div_ceil(256))?;
        let params = Params {
            sample_rate_hz,
            sample_count: sample_count.try_into().map_err(|_| GpuError::Shape)?,
            tone_count: tone_count.try_into().map_err(|_| GpuError::Shape)?,
            player_count: player_count.try_into().map_err(|_| GpuError::Shape)?,
        };

        let frequency_buffer = storage_buffer(&self.device, "frequencies", frequencies_hz);
        let amplitude_buffer = storage_buffer(&self.device, "amplitudes", amplitudes);
        let phase_buffer = storage_buffer(&self.device, "phases", phases);
        let output_size = (output_len * std::mem::size_of::<f32>()) as wgpu::BufferAddress;
        let output_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Qu multisine GPU output"),
            size: output_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let params_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Qu multisine parameters"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Qu multisine readback"),
            size: output_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Qu multisine bind group"),
            layout: &self.layout,
            entries: &[
                whole_entry(0, &frequency_buffer),
                whole_entry(1, &amplitude_buffer),
                whole_entry(2, &phase_buffer),
                whole_entry(3, &output_buffer),
                whole_entry(4, &params_buffer),
            ],
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Qu multisine command encoder"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Qu multisine compute pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups((output_len as u32).div_ceil(256), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output_buffer, 0, &staging, 0, output_size);
        self.queue.submit(Some(encoder.finish()));

        let slice = staging.slice(..);
        let (sender, receiver) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device.poll(wgpu::Maintain::Wait);
        receiver
            .recv()
            .map_err(|error| GpuError::Map(error.to_string()))?
            .map_err(|error| GpuError::Map(error.to_string()))?;
        let mapped = slice.get_mapped_range();
        let output = bytemuck::cast_slice::<u8, f32>(&mapped).to_vec();
        drop(mapped);
        staging.unmap();
        Ok(output)
    }

    /// Naive row-major matmul: `a` is `(m, k)`, `b` is `(k, n)`, returns
    /// `(m, n)`. One thread per output element, no tiling — a first real GPU
    /// kernel to prove the wiring works, not a tuned one; `qu-core`'s CPU
    /// `Matrix::matmul` remains the semantic oracle (see the module doc).
    /// Deliberately not wired into `qu-interp` yet: doing so would pull
    /// `wgpu`'s full dependency tree into the `engine/` workspace ahead of
    /// its declared milestone (IMPL.md's M6 Acceleration is still "Planned",
    /// gated behind M2-M5) — this stays a `qu-gpu`-internal capability with
    /// its own CPU-parity test until that milestone is actually reached.
    pub fn matmul(&self, a: &[f32], m: usize, k: usize, b: &[f32], n: usize) -> Result<Vec<f32>, GpuError> {
        if m == 0 || k == 0 || n == 0 || a.len() != m * k || b.len() != k * n {
            return Err(GpuError::MatmulShape);
        }
        self.check_buffer_size((a.len() * std::mem::size_of::<f32>()) as u64)?;
        self.check_buffer_size((b.len() * std::mem::size_of::<f32>()) as u64)?;
        let output_len = m * n;
        let output_size = (output_len * std::mem::size_of::<f32>()) as wgpu::BufferAddress;
        self.check_buffer_size(output_size)?;
        // `dispatch_workgroups(n.div_ceil(16), m.div_ceil(16), 1)` below —
        // a "tall and skinny" matmul (huge `m`, tiny `k`/`n`, e.g. millions
        // of particle-filter rows against a 4x4 state-transition matrix)
        // can drive `m.div_ceil(16)` past the device's dispatch-per-
        // dimension limit long before either buffer above gets anywhere
        // near ITS limit — confirmed directly: `particle_filter`'s own
        // `.move(dt)` GPU path crashed the process this way (`qu-interp`)
        // before this precheck existed, the exact same "uncaptured wgpu
        // validation error panics instead of returning Err" failure mode
        // `check_buffer_size` above already guards against, just for a
        // different device limit.
        self.check_workgroup_count("x (n)", (n as u32).div_ceil(16))?;
        self.check_workgroup_count("y (m)", (m as u32).div_ceil(16))?;
        let params = MatmulParams {
            m: m.try_into().map_err(|_| GpuError::MatmulShape)?,
            k: k.try_into().map_err(|_| GpuError::MatmulShape)?,
            n: n.try_into().map_err(|_| GpuError::MatmulShape)?,
            _pad: 0,
        };
        let a_buffer = storage_buffer(&self.device, "matmul a", a);
        let b_buffer = storage_buffer(&self.device, "matmul b", b);
        let c_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Qu matmul output"),
            size: output_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let params_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Qu matmul parameters"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Qu matmul readback"),
            size: output_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Qu matmul bind group"),
            layout: &self.matmul_layout,
            entries: &[
                whole_entry(0, &a_buffer),
                whole_entry(1, &b_buffer),
                whole_entry(2, &c_buffer),
                whole_entry(3, &params_buffer),
            ],
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Qu matmul command encoder"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Qu matmul compute pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.matmul_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups((n as u32).div_ceil(16), (m as u32).div_ceil(16), 1);
        }
        encoder.copy_buffer_to_buffer(&c_buffer, 0, &staging, 0, output_size);
        self.queue.submit(Some(encoder.finish()));

        let slice = staging.slice(..);
        let (sender, receiver) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device.poll(wgpu::Maintain::Wait);
        receiver
            .recv()
            .map_err(|error| GpuError::Map(error.to_string()))?
            .map_err(|error| GpuError::Map(error.to_string()))?;
        let mapped = slice.get_mapped_range();
        let output = bytemuck::cast_slice::<u8, f32>(&mapped).to_vec();
        drop(mapped);
        staging.unmap();
        Ok(output)
    }
}

fn storage_entry(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn storage_buffer<T: Pod>(device: &wgpu::Device, label: &str, values: &[T]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::cast_slice(values),
        usage: wgpu::BufferUsages::STORAGE,
    })
}

fn whole_entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}
