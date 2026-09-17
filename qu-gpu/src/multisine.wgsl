struct Params {
    sample_rate_hz: f32,
    sample_count: u32,
    tone_count: u32,
    player_count: u32,
}

@group(0) @binding(0) var<storage, read> frequencies_hz: array<f32>;
@group(0) @binding(1) var<storage, read> amplitudes: array<f32>;
@group(0) @binding(2) var<storage, read> phases: array<f32>;
@group(0) @binding(3) var<storage, read_write> output: array<f32>;
@group(0) @binding(4) var<uniform> params: Params;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let output_count = params.player_count * params.sample_count;
    if id.x >= output_count {
        return;
    }

    let player = id.x / params.sample_count;
    let sample = id.x % params.sample_count;
    let time = f32(sample) / params.sample_rate_hz;
    var value = 0.0;
    var tone = 0u;
    loop {
        if tone >= params.tone_count {
            break;
        }
        let phase = phases[player * params.tone_count + tone];
        value += amplitudes[tone] * cos(6.283185307179586 * frequencies_hz[tone] * time + phase);
        tone += 1u;
    }
    output[id.x] = value;
}
