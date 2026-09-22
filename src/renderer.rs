use crate::mesh::Mesh;
use glam::{Vec2, Vec3};
use std::fmt::Write as _;

const NEAR: f32 = 0.12;
const FAR: f32 = 170.0;
const CELL_ASPECT: f32 = 0.5; // Terminal cells are about twice as tall as wide.
const PALETTE: &[u8] = b" .:-=+*#%@";

#[derive(Clone, Copy)]
pub struct Camera {
    pub position: Vec3,
    pub target: Vec3,
}

#[derive(Clone, Copy)]
pub struct Cell {
    pub ch: char,
    pub depth: f32,
    pub color: (u8, u8, u8),
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            depth: f32::INFINITY,
            color: (0, 0, 0),
        }
    }
}

#[derive(Clone, Copy)]
struct Vertex {
    pos: Vec3,
}

pub struct Renderer {
    pub width: usize,
    pub height: usize,
    cells: Vec<Cell>,
    color: bool,
}

impl Renderer {
    pub fn new(width: usize, height: usize, color: bool) -> Self {
        Self {
            width,
            height,
            cells: vec![Cell::default(); width * height],
            color,
        }
    }

    pub fn clear(&mut self) {
        self.cells.fill(Cell::default());
    }

    pub fn text(&mut self, x: usize, y: usize, value: &str) {
        if y >= self.height {
            return;
        }
        for (i, ch) in value.chars().enumerate() {
            if x + i >= self.width {
                break;
            }
            self.cells[y * self.width + x + i] = Cell {
                ch,
                depth: -1.0,
                color: (255, 255, 255),
            };
        }
    }

    pub fn draw_mesh(&mut self, mesh: &Mesh, camera: Camera) {
        let forward = (camera.target - camera.position).normalize_or_zero();
        let right = Vec3::Y.cross(forward).normalize_or_zero();
        let up = forward.cross(right);
        let light = Vec3::new(-0.35, 0.85, -0.4).normalize();
        for triangle in &mesh.triangles {
            let world = triangle.vertices;
            let normal = (world[1] - world[0])
                .cross(world[2] - world[0])
                .normalize_or_zero();
            let to_view = |p: Vec3| {
                let d = p - camera.position;
                Vertex {
                    pos: Vec3::new(d.dot(right), d.dot(up), d.dot(forward)),
                }
            };
            let points = world.map(to_view);
            let face = (points[1].pos - points[0].pos).cross(points[2].pos - points[0].pos);
            // In camera space, visible outward faces point towards the origin.
            if face.dot(points[0].pos) >= 0.0 {
                continue;
            }
            let brightness = (0.22 + 0.78 * normal.dot(light).max(0.0)) * triangle.shade;
            let clipped = clip_near(&points);
            if clipped.len() < 3 {
                continue;
            }
            for i in 1..clipped.len() - 1 {
                self.rasterize(
                    [clipped[0], clipped[i], clipped[i + 1]],
                    brightness,
                    triangle.color,
                );
            }
        }
    }

    fn rasterize(&mut self, vertices: [Vertex; 3], brightness: f32, color: (u8, u8, u8)) {
        let scale = self.height as f32 / (2.0 * (70.0_f32.to_radians() * 0.5).tan());
        let screen = vertices.map(|v| {
            Vec2::new(
                self.width as f32 * 0.5 + v.pos.x / v.pos.z * scale / CELL_ASPECT,
                self.height as f32 * 0.5 - v.pos.y / v.pos.z * scale,
            )
        });
        let edge =
            |a: Vec2, b: Vec2, p: Vec2| (p.x - a.x) * (b.y - a.y) - (p.y - a.y) * (b.x - a.x);
        let area = edge(screen[0], screen[1], screen[2]);
        if area.abs() < 0.001 {
            return;
        }
        let min_x = screen
            .iter()
            .map(|p| p.x)
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as usize;
        let max_x = screen
            .iter()
            .map(|p| p.x)
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .min(self.width as f32 - 1.0) as usize;
        let min_y = screen
            .iter()
            .map(|p| p.y)
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as usize;
        let max_y = screen
            .iter()
            .map(|p| p.y)
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .min(self.height as f32 - 1.0) as usize;
        let index = (brightness.clamp(0.0, 1.0) * (PALETTE.len() - 1) as f32).round() as usize;
        let ch = PALETTE[index] as char;
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let weights = [
                    edge(screen[1], screen[2], p) / area,
                    edge(screen[2], screen[0], p) / area,
                    edge(screen[0], screen[1], p) / area,
                ];
                if weights.iter().any(|&w| w < -0.0001) {
                    continue;
                }
                // Interpolating reciprocal depth keeps the Z-buffer correct under perspective.
                let inv_z = (0..3).map(|i| weights[i] / vertices[i].pos.z).sum::<f32>();
                if inv_z <= 0.0 {
                    continue;
                }
                let z = 1.0 / inv_z;
                if z > FAR {
                    continue;
                }
                let cell = &mut self.cells[y * self.width + x];
                if z < cell.depth {
                    *cell = Cell {
                        ch,
                        depth: z,
                        color,
                    };
                }
            }
        }
    }

    pub fn frame_string(&self) -> String {
        let mut out = String::with_capacity(self.width * self.height * 2 + 128);
        out.push_str("\x1b[H");
        let mut current = None;
        for row in self.cells.chunks(self.width) {
            for cell in row {
                if self.color && current != Some(cell.color) {
                    let (r, g, b) = cell.color;
                    let _ = write!(out, "\x1b[38;2;{r};{g};{b}m");
                    current = Some(cell.color);
                }
                out.push(cell.ch);
            }
            out.push_str("\x1b[0m\r\n");
            current = None;
        }
        out
    }
}

fn clip_near(input: &[Vertex; 3]) -> Vec<Vertex> {
    let mut output = Vec::with_capacity(4);
    for i in 0..3 {
        let a = input[i];
        let b = input[(i + 1) % 3];
        let inside_a = a.pos.z >= NEAR;
        let inside_b = b.pos.z >= NEAR;
        if inside_a {
            output.push(a);
        }
        if inside_a != inside_b {
            let t = (NEAR - a.pos.z) / (b.pos.z - a.pos.z);
            output.push(Vertex {
                pos: a.pos.lerp(b.pos, t),
            });
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::Mesh;
    use crate::{car::Car, track::Track};
    use glam::{Mat4, Vec3};

    #[test]
    fn cube_renders_and_camera_changes_image() {
        let cube = Mesh::box_mesh(Vec3::splat(2.0), (255, 255, 255));
        let mut scene = Mesh::new();
        scene.append_transformed(&cube, Mat4::IDENTITY);
        let mut renderer = Renderer::new(80, 35, false);
        let cam = Camera {
            position: Vec3::new(3.0, 2.0, -6.0),
            target: Vec3::ZERO,
        };
        renderer.draw_mesh(&scene, cam);
        let first = renderer.frame_string();
        assert!(first.chars().any(|c| "=+*#%@".contains(c)));
        renderer.clear();
        renderer.draw_mesh(
            &scene,
            Camera {
                position: Vec3::new(-3.0, 2.0, -6.0),
                ..cam
            },
        );
        assert_ne!(first, renderer.frame_string());
    }

    #[test]
    fn near_plane_clips_triangles() {
        let tri = [
            Vertex {
                pos: Vec3::new(0.0, 0.0, 0.01),
            },
            Vertex {
                pos: Vec3::new(1.0, 0.0, 1.0),
            },
            Vertex {
                pos: Vec3::new(0.0, 1.0, 1.0),
            },
        ];
        assert_eq!(clip_near(&tri).len(), 4);
    }

    #[test]
    fn nearer_triangle_wins_independent_of_draw_order() {
        let mut renderer = Renderer::new(50, 25, false);
        let make = |z| {
            [
                Vertex {
                    pos: Vec3::new(-1.0, -1.0, z),
                },
                Vertex {
                    pos: Vec3::new(1.0, -1.0, z),
                },
                Vertex {
                    pos: Vec3::new(0.0, 1.0, z),
                },
            ]
        };
        renderer.rasterize(make(3.0), 0.8, (255, 0, 0));
        renderer.rasterize(make(5.0), 0.2, (0, 0, 255));
        let cell = renderer.cells[12 * 50 + 25];
        assert_eq!(cell.color, (255, 0, 0));
        assert!((cell.depth - 3.0).abs() < 0.01);
    }

    #[test]
    fn car_is_visible_over_the_track() {
        let mut renderer = Renderer::new(109, 33, true);
        let track = Track::new();
        let car = Car::new();
        let camera = Camera {
            position: Vec3::new(0.0, 3.7, -6.3),
            target: Vec3::new(0.0, 0.8, 3.0),
        };
        renderer.draw_mesh(&track.mesh, camera);
        renderer.draw_mesh(&car.mesh(), camera);
        let red_cells = renderer
            .cells
            .iter()
            .filter(|c| c.color == (245, 65, 52))
            .count();
        assert!(red_cells > 10, "only {red_cells} car body cells visible");
    }
}
