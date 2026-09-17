//! Mesh arrays and deterministic collision-hull compilation.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    pub fn lerp(self, other: Self, amount: f64) -> Self {
        Self::new(
            self.x + (other.x - self.x) * amount,
            self.y + (other.y - self.y) * amount,
            self.z + (other.z - self.z) * amount,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds3 {
    pub min: Vec3,
    pub max: Vec3,
}

impl Bounds3 {
    pub fn size(self) -> Vec3 {
        Vec3::new(
            self.max.x - self.min.x,
            self.max.y - self.min.y,
            self.max.z - self.min.z,
        )
    }
}

/// Indexed triangle mesh: positions are an `n x 3` array and faces an `m x 3` array.
#[derive(Clone, Debug, PartialEq)]
pub struct Mesh3 {
    pub vertices: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MeshError {
    EmptyVertices,
    NonFiniteVertex { index: usize },
    IndexOutOfBounds { triangle: usize, index: u32 },
    DegenerateTriangle { triangle: usize },
    InvalidSize,
    InvalidTessellation,
}

impl Display for MeshError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyVertices => write!(formatter, "mesh requires at least one vertex"),
            Self::NonFiniteVertex { index } => write!(formatter, "vertex {index} is not finite"),
            Self::IndexOutOfBounds { triangle, index } => {
                write!(
                    formatter,
                    "triangle {triangle} references missing vertex {index}"
                )
            }
            Self::DegenerateTriangle { triangle } => {
                write!(formatter, "triangle {triangle} repeats a vertex index")
            }
            Self::InvalidSize => write!(
                formatter,
                "primitive dimensions must be finite and positive"
            ),
            Self::InvalidTessellation => {
                write!(formatter, "primitive tessellation is below its minimum")
            }
        }
    }
}

impl Error for MeshError {}

impl Mesh3 {
    pub fn new(vertices: Vec<Vec3>, triangles: Vec<[u32; 3]>) -> Result<Self, MeshError> {
        let mesh = Self {
            vertices,
            triangles,
        };
        mesh.validate()?;
        Ok(mesh)
    }

    pub fn from_arrays(vertices: &[[f64; 3]], triangles: &[[u32; 3]]) -> Result<Self, MeshError> {
        Self::new(
            vertices
                .iter()
                .map(|value| Vec3::new(value[0], value[1], value[2]))
                .collect(),
            triangles.to_vec(),
        )
    }

    pub fn validate(&self) -> Result<(), MeshError> {
        if self.vertices.is_empty() {
            return Err(MeshError::EmptyVertices);
        }
        for (index, vertex) in self.vertices.iter().enumerate() {
            if !vertex.is_finite() {
                return Err(MeshError::NonFiniteVertex { index });
            }
        }
        for (triangle, indices) in self.triangles.iter().enumerate() {
            if indices[0] == indices[1] || indices[1] == indices[2] || indices[0] == indices[2] {
                return Err(MeshError::DegenerateTriangle { triangle });
            }
            for index in indices {
                if *index as usize >= self.vertices.len() {
                    return Err(MeshError::IndexOutOfBounds {
                        triangle,
                        index: *index,
                    });
                }
            }
        }
        Ok(())
    }

    pub fn bounds(&self) -> Bounds3 {
        let mut min = self.vertices[0];
        let mut max = self.vertices[0];
        for vertex in &self.vertices[1..] {
            min.x = min.x.min(vertex.x);
            min.y = min.y.min(vertex.y);
            min.z = min.z.min(vertex.z);
            max.x = max.x.max(vertex.x);
            max.y = max.y.max(vertex.y);
            max.z = max.z.max(vertex.z);
        }
        Bounds3 { min, max }
    }

    pub fn box_mesh(size: Vec3) -> Result<Self, MeshError> {
        if !size.is_finite() || size.x <= 0.0 || size.y <= 0.0 || size.z <= 0.0 {
            return Err(MeshError::InvalidSize);
        }
        let half = Vec3::new(size.x / 2.0, size.y / 2.0, size.z / 2.0);
        Ok(Self {
            vertices: box_vertices(Vec3::new(-half.x, -half.y, -half.z), half),
            triangles: box_triangles(),
        })
    }

    pub fn plane(width: f64, depth: f64) -> Result<Self, MeshError> {
        if !width.is_finite() || !depth.is_finite() || width <= 0.0 || depth <= 0.0 {
            return Err(MeshError::InvalidSize);
        }
        let x = width / 2.0;
        let z = depth / 2.0;
        Ok(Self {
            vertices: vec![
                Vec3::new(-x, 0.0, -z),
                Vec3::new(x, 0.0, -z),
                Vec3::new(x, 0.0, z),
                Vec3::new(-x, 0.0, z),
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
        })
    }

    pub fn tetrahedron(radius: f64) -> Result<Self, MeshError> {
        if !radius.is_finite() || radius <= 0.0 {
            return Err(MeshError::InvalidSize);
        }
        let scale = radius / 3.0_f64.sqrt();
        Ok(Self {
            vertices: vec![
                Vec3::new(scale, scale, scale),
                Vec3::new(-scale, -scale, scale),
                Vec3::new(-scale, scale, -scale),
                Vec3::new(scale, -scale, -scale),
            ],
            triangles: vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
        })
    }

    /// Premade UV sphere. `segments` goes around the equator and `rings` is the
    /// number of intermediate latitude rings between the two poles.
    pub fn uv_sphere(radius: f64, segments: usize, rings: usize) -> Result<Self, MeshError> {
        if !radius.is_finite() || radius <= 0.0 {
            return Err(MeshError::InvalidSize);
        }
        if segments < 3 || rings < 1 {
            return Err(MeshError::InvalidTessellation);
        }

        let mut vertices = Vec::with_capacity(2 + segments * rings);
        vertices.push(Vec3::new(0.0, radius, 0.0));
        for ring in 1..=rings {
            let latitude = std::f64::consts::PI * ring as f64 / (rings + 1) as f64;
            let y = radius * latitude.cos();
            let circle = radius * latitude.sin();
            for segment in 0..segments {
                let longitude = std::f64::consts::TAU * segment as f64 / segments as f64;
                vertices.push(Vec3::new(
                    circle * longitude.cos(),
                    y,
                    circle * longitude.sin(),
                ));
            }
        }
        let bottom = vertices.len() as u32;
        vertices.push(Vec3::new(0.0, -radius, 0.0));

        let mut triangles = Vec::with_capacity(segments * rings * 2);
        for segment in 0..segments {
            let next = (segment + 1) % segments;
            triangles.push([0, 1 + next as u32, 1 + segment as u32]);
        }
        for ring in 0..rings.saturating_sub(1) {
            let current = 1 + ring * segments;
            let next_ring = current + segments;
            for segment in 0..segments {
                let next = (segment + 1) % segments;
                let a = (current + segment) as u32;
                let b = (current + next) as u32;
                let c = (next_ring + segment) as u32;
                let d = (next_ring + next) as u32;
                triangles.push([a, b, d]);
                triangles.push([a, d, c]);
            }
        }
        let last_ring = 1 + (rings - 1) * segments;
        for segment in 0..segments {
            let next = (segment + 1) % segments;
            triangles.push([
                bottom,
                (last_ring + segment) as u32,
                (last_ring + next) as u32,
            ]);
        }
        Self::new(vertices, triangles)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HullMode {
    /// Fast broad-phase hull with eight vertices and twelve triangles.
    Box,
    /// Exact indexed mesh for static colliders; dynamic bodies should prefer a convex provider.
    TriangleMesh,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CollisionHull {
    pub mode: HullMode,
    pub vertices: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    pub bounds: Bounds3,
    /// Deterministic fingerprint suitable for build caches and asset manifests.
    pub fingerprint: u64,
}

pub fn compile_hull(mesh: &Mesh3, mode: HullMode) -> Result<CollisionHull, MeshError> {
    mesh.validate()?;
    let bounds = mesh.bounds();
    let (vertices, triangles) = match mode {
        HullMode::Box => (box_vertices(bounds.min, bounds.max), box_triangles()),
        HullMode::TriangleMesh => (mesh.vertices.clone(), mesh.triangles.clone()),
    };
    let fingerprint = fingerprint(mode, &vertices, &triangles);
    Ok(CollisionHull {
        mode,
        vertices,
        triangles,
        bounds,
        fingerprint,
    })
}

fn box_vertices(min: Vec3, max: Vec3) -> Vec<Vec3> {
    vec![
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(max.x, max.y, max.z),
        Vec3::new(min.x, max.y, max.z),
    ]
}

fn box_triangles() -> Vec<[u32; 3]> {
    vec![
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [3, 7, 6],
        [3, 6, 2],
        [0, 4, 7],
        [0, 7, 3],
        [1, 2, 6],
        [1, 6, 5],
    ]
}

fn fingerprint(mode: HullMode, vertices: &[Vec3], triangles: &[[u32; 3]]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    let mode_byte = match mode {
        HullMode::Box => 1,
        HullMode::TriangleMesh => 2,
    };
    hash_byte(&mut hash, mode_byte);
    for vertex in vertices {
        for value in [vertex.x, vertex.y, vertex.z] {
            for byte in value.to_bits().to_le_bytes() {
                hash_byte(&mut hash, byte);
            }
        }
    }
    for triangle in triangles {
        for index in triangle {
            for byte in index.to_le_bytes() {
                hash_byte(&mut hash, byte);
            }
        }
    }
    hash
}

fn hash_byte(hash: &mut u64, byte: u8) {
    *hash ^= byte as u64;
    *hash = hash.wrapping_mul(0x100000001b3);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrays_are_validated_before_becoming_a_mesh() {
        let error = Mesh3::from_arrays(&[[0.0, 0.0, 0.0]], &[[0, 1, 2]]).unwrap_err();
        assert_eq!(
            error,
            MeshError::IndexOutOfBounds {
                triangle: 0,
                index: 1
            }
        );
    }

    #[test]
    fn premade_box_has_expected_arrays() {
        let mesh = Mesh3::box_mesh(Vec3::new(2.0, 4.0, 6.0)).unwrap();
        assert_eq!(mesh.vertices.len(), 8);
        assert_eq!(mesh.triangles.len(), 12);
        assert_eq!(mesh.bounds().size(), Vec3::new(2.0, 4.0, 6.0));
    }

    #[test]
    fn premade_sphere_has_predictable_tessellation() {
        let sphere = Mesh3::uv_sphere(2.0, 12, 6).unwrap();
        assert_eq!(sphere.vertices.len(), 2 + 12 * 6);
        assert_eq!(sphere.triangles.len(), 12 * 6 * 2);
        let bounds = sphere.bounds();
        assert_eq!(bounds.min.y, -2.0);
        assert_eq!(bounds.max.y, 2.0);
        sphere.validate().unwrap();
    }

    #[test]
    fn box_hull_compiles_to_a_stable_physics_artifact() {
        let mesh = Mesh3::tetrahedron(2.0).unwrap();
        let first = compile_hull(&mesh, HullMode::Box).unwrap();
        let second = compile_hull(&mesh, HullMode::Box).unwrap();
        assert_eq!(first.vertices.len(), 8);
        assert_eq!(first.triangles.len(), 12);
        assert_eq!(first.fingerprint, second.fingerprint);
    }

    #[test]
    fn triangle_hull_preserves_static_mesh_indices() {
        let mesh = Mesh3::plane(10.0, 8.0).unwrap();
        let hull = compile_hull(&mesh, HullMode::TriangleMesh).unwrap();
        assert_eq!(hull.vertices, mesh.vertices);
        assert_eq!(hull.triangles, mesh.triangles);
    }
}
