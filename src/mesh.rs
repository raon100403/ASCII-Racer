use glam::{Mat4, Vec3};

#[derive(Clone, Copy)]
pub struct Triangle {
    pub vertices: [Vec3; 3],
    pub color: (u8, u8, u8),
    pub shade: f32,
}

pub struct Mesh {
    pub triangles: Vec<Triangle>,
}

impl Mesh {
    pub fn new() -> Self {
        Self {
            triangles: Vec::new(),
        }
    }

    pub fn add(&mut self, vertices: [Vec3; 3], color: (u8, u8, u8), shade: f32) {
        self.triangles.push(Triangle {
            vertices,
            color,
            shade,
        });
    }

    pub fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, color: (u8, u8, u8), shade: f32) {
        self.add([a, b, c], color, shade);
        self.add([a, c, d], color, shade);
    }

    pub fn box_mesh(size: Vec3, color: (u8, u8, u8)) -> Self {
        let mut mesh = Self::new();
        let (x, y, z) = (size.x * 0.5, size.y * 0.5, size.z * 0.5);
        let p = [
            Vec3::new(-x, -y, -z),
            Vec3::new(x, -y, -z),
            Vec3::new(x, y, -z),
            Vec3::new(-x, y, -z),
            Vec3::new(-x, -y, z),
            Vec3::new(x, -y, z),
            Vec3::new(x, y, z),
            Vec3::new(-x, y, z),
        ];
        // Winding points outwards; back faces are removed by the renderer.
        for [a, b, c, d] in [
            [4, 5, 6, 7],
            [1, 0, 3, 2],
            [0, 4, 7, 3],
            [5, 1, 2, 6],
            [3, 7, 6, 2],
            [0, 1, 5, 4],
        ] {
            mesh.quad(p[a], p[b], p[c], p[d], color, 1.0);
        }
        mesh
    }

    pub fn append_transformed(&mut self, other: &Mesh, transform: Mat4) {
        for tri in &other.triangles {
            self.add(
                tri.vertices.map(|v| transform.transform_point3(v)),
                tri.color,
                tri.shade,
            );
        }
    }
}
