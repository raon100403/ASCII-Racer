use crate::mesh::{Mesh, SurfaceKind};
use glam::{Vec2, Vec3};
use std::fmt::Write as _;

const NEAR: f32 = 0.12;
const FAR: f32 = 170.0;
const SAMPLE_OFFSETS: [(f32, f32); 4] = [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)];
const PALETTE: &[u8] = b" .:-=+*#%@";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AaMode {
    Off,
    X2,
}

impl AaMode {
    fn count(self) -> usize {
        match self {
            Self::Off => 1,
            Self::X2 => 4,
        }
    }
}

#[derive(Clone, Copy)]
struct Sample {
    depth: f32,
    color: (u8, u8, u8),
    brightness: f32,
    coverage: f32,
    surface: SurfaceKind,
}

impl Default for Sample {
    fn default() -> Self {
        Self {
            depth: f32::INFINITY,
            color: (0, 0, 0),
            brightness: 0.0,
            coverage: 0.0,
            surface: SurfaceKind::Generic,
        }
    }
}

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
    samples: Vec<Sample>,
    color: bool,
    cell_aspect: f32,
    aa: AaMode,
}

impl Renderer {
    pub fn new(width: usize, height: usize, color: bool) -> Self {
        Self::with_options(width, height, color, 0.5, AaMode::X2)
    }

    pub fn with_options(
        width: usize,
        height: usize,
        color: bool,
        cell_aspect: f32,
        aa: AaMode,
    ) -> Self {
        assert!(cell_aspect.is_finite() && cell_aspect > 0.0);
        Self {
            width,
            height,
            cells: vec![Cell::default(); width * height],
            samples: vec![Sample::default(); width * height * aa.count()],
            color,
            cell_aspect,
            aa,
        }
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
        self.cells.resize(width * height, Cell::default());
        self.samples
            .resize(width * height * self.aa.count(), Sample::default());
        self.clear();
    }

    pub fn clear(&mut self) {
        self.cells.fill(Cell::default());
        self.samples.fill(Sample::default());
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
            let (clipped, len) = clip_near(&points);
            if len < 3 {
                continue;
            }
            for i in 1..len - 1 {
                self.rasterize(
                    [clipped[0], clipped[i], clipped[i + 1]],
                    brightness,
                    triangle.color,
                    triangle.surface,
                );
            }
        }
    }

    fn rasterize(
        &mut self,
        vertices: [Vertex; 3],
        brightness: f32,
        color: (u8, u8, u8),
        surface: SurfaceKind,
    ) {
        let scale = self.height as f32 / (2.0 * (70.0_f32.to_radians() * 0.5).tan());
        let screen = vertices.map(|v| {
            Vec2::new(
                self.width as f32 * 0.5 + v.pos.x / v.pos.z * scale / self.cell_aspect,
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
        let inv_depth = vertices.map(|v| v.pos.z.recip());
        let count = self.aa.count();
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let mut hit = false;
                for sample_index in 0..count {
                    let (dx, dy) = if count == 1 {
                        (0.5, 0.5)
                    } else {
                        SAMPLE_OFFSETS[sample_index]
                    };
                    let p = Vec2::new(x as f32 + dx, y as f32 + dy);
                    let w0 = edge(screen[1], screen[2], p) / area;
                    let w1 = edge(screen[2], screen[0], p) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -0.0001 || w1 < -0.0001 || w2 < -0.0001 {
                        continue;
                    }
                    let inv_z = w0 * inv_depth[0] + w1 * inv_depth[1] + w2 * inv_depth[2];
                    if inv_z <= 0.0 {
                        continue;
                    }
                    let z = inv_z.recip();
                    if z > FAR {
                        continue;
                    }
                    hit = true;
                    let sample = &mut self.samples[(y * self.width + x) * count + sample_index];
                    if z < sample.depth {
                        *sample = Sample {
                            depth: z,
                            color,
                            brightness,
                            surface,
                            coverage: 1.0,
                        };
                    }
                }
                // A long distant paint strip can pass between all four point samples.
                // Clip only such misses against the cell; keep a fractional, depth-tested
                // contribution rather than inflating its world-space width.
                if count == 4 && surface == SurfaceKind::RoadMarking && !hit {
                    if let Some((centroid, area_covered)) = triangle_cell_overlap(screen, x, y) {
                        let w0 = edge(screen[1], screen[2], centroid) / area;
                        let w1 = edge(screen[2], screen[0], centroid) / area;
                        let w2 = 1.0 - w0 - w1;
                        let inv_z = w0 * inv_depth[0] + w1 * inv_depth[1] + w2 * inv_depth[2];
                        if inv_z > 0.0 {
                            let z = inv_z.recip();
                            if z <= FAR {
                                let index = SAMPLE_OFFSETS
                                    .iter()
                                    .enumerate()
                                    .min_by(|(_, a), (_, b)| {
                                        let distance = |offset: &(f32, f32)| {
                                            (centroid
                                                - Vec2::new(
                                                    x as f32 + offset.0,
                                                    y as f32 + offset.1,
                                                ))
                                            .length_squared()
                                        };
                                        distance(a).total_cmp(&distance(b))
                                    })
                                    .unwrap()
                                    .0;
                                let sample =
                                    &mut self.samples[(y * self.width + x) * count + index];
                                if z < sample.depth {
                                    *sample = Sample {
                                        depth: z,
                                        color,
                                        brightness,
                                        surface,
                                        coverage: (area_covered * count as f32).min(1.0),
                                    };
                                } else if sample.surface == surface
                                    && sample.color == color
                                    && (sample.depth - z).abs() < z * 0.01
                                {
                                    sample.coverage =
                                        (sample.coverage + area_covered * count as f32).min(1.0);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn resolve(&mut self) {
        let count = self.aa.count();
        for (cell, samples) in self.cells.iter_mut().zip(self.samples.chunks_exact(count)) {
            // Text is an overlay; it must not be replaced by the 3D resolve.
            if cell.depth < 0.0 {
                continue;
            }
            let mut winner = None;
            let mut best_score = 0.0;
            let nearest_depth = samples.iter().fold(f32::INFINITY, |z, s| z.min(s.depth));
            for candidate in samples.iter().filter(|s| s.depth.is_finite()) {
                // A genuinely nearer surface occludes even an important distant marking.
                // Nearby coplanar road/paint samples may still compete by coverage.
                if candidate.depth > nearest_depth * 1.08 {
                    continue;
                }
                let covered = samples
                    .iter()
                    .filter(|s| {
                        s.surface == candidate.surface
                            && s.color == candidate.color
                            && s.depth.is_finite()
                            && s.depth <= candidate.depth * 1.08
                            && candidate.depth <= s.depth * 1.08
                    })
                    .count();
                let score = covered as f32
                    * if candidate.surface.important() {
                        3.2
                    } else {
                        1.0
                    };
                if score > best_score
                    || (score == best_score
                        && winner.is_none_or(|w: Sample| candidate.depth < w.depth))
                {
                    best_score = score;
                    winner = Some(*candidate);
                }
            }
            if let Some(sample) = winner {
                let coverage: f32 = samples
                    .iter()
                    .filter(|s| {
                        s.surface == sample.surface
                            && s.color == sample.color
                            && s.depth.is_finite()
                            && s.depth <= sample.depth * 1.08
                            && sample.depth <= s.depth * 1.08
                    })
                    .map(|s| s.coverage)
                    .sum::<f32>()
                    / count as f32;
                let lighting = sample.brightness.clamp(0.0, 1.0);
                // Coverage carries silhouettes; RGB carries most of the lighting.
                // Keep fully covered ground and road in the lighter ASCII ranks.
                let intensity =
                    (coverage * (0.12 + 0.40 * lighting)).max(if sample.surface.important() {
                        0.17
                    } else {
                        0.0
                    });
                let index = (intensity * (PALETTE.len() - 1) as f32).round() as usize;
                let color_scale = (0.38 + 0.62 * lighting) * (0.72 + 0.28 * coverage);
                let lit =
                    |channel: u8| (channel as f32 * color_scale).round().clamp(0.0, 255.0) as u8;
                *cell = Cell {
                    ch: PALETTE[index] as char,
                    depth: sample.depth,
                    color: (
                        lit(sample.color.0),
                        lit(sample.color.1),
                        lit(sample.color.2),
                    ),
                };
            } else {
                *cell = Cell::default();
            }
        }
    }

    pub fn frame_string(&mut self) -> String {
        self.resolve();
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

    /// Packs each cell as `0xBBGGRRCC` for the dependency-free WebAssembly host.
    /// `CC` is the ASCII character and the remaining bytes are its RGB color.
    pub fn write_packed_cells(&mut self, output: &mut Vec<u32>) {
        self.resolve();
        output.clear();
        output.reserve(self.cells.len());
        output.extend(self.cells.iter().map(|cell| {
            cell.ch as u32
                | (cell.color.0 as u32) << 8
                | (cell.color.1 as u32) << 16
                | (cell.color.2 as u32) << 24
        }));
    }
}

fn clip_near(input: &[Vertex; 3]) -> ([Vertex; 4], usize) {
    let mut output = [input[0]; 4];
    let mut len = 0;
    for i in 0..3 {
        let a = input[i];
        let b = input[(i + 1) % 3];
        let inside_a = a.pos.z >= NEAR;
        if inside_a {
            output[len] = a;
            len += 1;
        }
        if inside_a != (b.pos.z >= NEAR) {
            let t = (NEAR - a.pos.z) / (b.pos.z - a.pos.z);
            output[len] = Vertex {
                pos: a.pos.lerp(b.pos, t),
            };
            len += 1;
        }
    }
    (output, len)
}

// Area and centroid of a projected triangle clipped to one terminal cell.
// This is only used for missed road paint; the hot path stays four point tests.
fn triangle_cell_overlap(triangle: [Vec2; 3], x: usize, y: usize) -> Option<(Vec2, f32)> {
    let mut polygon = [Vec2::ZERO; 8];
    polygon[..3].copy_from_slice(&triangle);
    let mut len = 3;
    let mut next = [Vec2::ZERO; 8];
    for side in 0..4 {
        let distance = |p: Vec2| match side {
            0 => p.x - x as f32,
            1 => x as f32 + 1.0 - p.x,
            2 => p.y - y as f32,
            _ => y as f32 + 1.0 - p.y,
        };
        let mut n = 0;
        for i in 0..len {
            let a = polygon[i];
            let b = polygon[(i + 1) % len];
            let da = distance(a);
            let db = distance(b);
            if (da >= 0.0) != (db >= 0.0) {
                next[n] = a.lerp(b, da / (da - db));
                n += 1;
            }
            if db >= 0.0 {
                next[n] = b;
                n += 1;
            }
        }
        if n < 3 {
            return None;
        }
        std::mem::swap(&mut polygon, &mut next);
        len = n;
    }
    let mut twice_area = 0.0;
    let mut centroid = Vec2::ZERO;
    for i in 0..len {
        let a = polygon[i];
        let b = polygon[(i + 1) % len];
        let cross = a.perp_dot(b);
        twice_area += cross;
        centroid += a;
    }
    let area = twice_area.abs() * 0.5;
    if area < 0.01 {
        return None;
    }
    Some((centroid / len as f32, area.min(1.0)))
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
        assert_eq!(clip_near(&tri).1, 4);
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
        renderer.rasterize(make(3.0), 0.8, (255, 0, 0), SurfaceKind::Generic);
        renderer.rasterize(make(5.0), 0.2, (0, 0, 255), SurfaceKind::Generic);
        renderer.resolve();
        let cell = renderer.cells[12 * 50 + 25];
        assert!(cell.color.0 > 0 && cell.color.1 == 0 && cell.color.2 == 0);
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
        renderer.resolve();
        let red_cells = renderer
            .cells
            .iter()
            .filter(|c| {
                c.ch != ' '
                    && (c.color.0 as u16) > (c.color.1 as u16) * 2
                    && (c.color.0 as u16) > (c.color.2 as u16) * 2
            })
            .count();
        assert!(
            red_cells > 10,
            "only {red_cells} red car body cells visible"
        );
    }

    #[test]
    fn subcell_marking_survives_without_center_coverage() {
        let mut renderer = Renderer::new(50, 25, false);
        let a = Vertex {
            pos: Vec3::new(0.018, -0.6, 5.0),
        };
        let b = Vertex {
            pos: Vec3::new(0.05, -0.6, 5.0),
        };
        let c = Vertex {
            pos: Vec3::new(0.05, 0.6, 5.0),
        };
        let d = Vertex {
            pos: Vec3::new(0.018, 0.6, 5.0),
        };
        renderer.rasterize([a, b, c], 1.0, (245, 225, 145), SurfaceKind::RoadMarking);
        renderer.rasterize([a, c, d], 1.0, (245, 225, 145), SurfaceKind::RoadMarking);
        renderer.resolve();
        assert_ne!(renderer.cells[12 * 50 + 25].ch, ' ');
        assert_ne!(renderer.cells[12 * 50 + 25].ch, '@');
        assert_eq!(
            renderer.samples[(12 * 50 + 25) * 4..(12 * 50 + 26) * 4]
                .iter()
                .filter(|s| s.depth.is_finite())
                .count(),
            2
        );
        let mut off = Renderer::with_options(50, 25, false, 0.5, AaMode::Off);
        off.rasterize([a, b, c], 1.0, (245, 225, 145), SurfaceKind::RoadMarking);
        off.rasterize([a, c, d], 1.0, (245, 225, 145), SurfaceKind::RoadMarking);
        off.resolve();
        assert_eq!(off.cells[12 * 50 + 25].ch, ' ');
    }

    #[test]
    fn nearer_surface_hides_thin_marking_even_when_drawn_first() {
        let mut renderer = Renderer::new(50, 25, true);
        let marking = [
            Vertex {
                pos: Vec3::new(0.018, -0.6, 5.0),
            },
            Vertex {
                pos: Vec3::new(0.05, -0.6, 5.0),
            },
            Vertex {
                pos: Vec3::new(0.05, 0.6, 5.0),
            },
        ];
        let foreground = [
            Vertex {
                pos: Vec3::new(-1.0, -1.0, 3.0),
            },
            Vertex {
                pos: Vec3::new(1.0, -1.0, 3.0),
            },
            Vertex {
                pos: Vec3::new(0.0, 1.0, 3.0),
            },
        ];
        for near_first in [false, true] {
            renderer.clear();
            if near_first {
                renderer.rasterize(foreground, 0.8, (255, 0, 0), SurfaceKind::Vehicle);
            }
            renderer.rasterize(marking, 1.0, (245, 225, 145), SurfaceKind::RoadMarking);
            if !near_first {
                renderer.rasterize(foreground, 0.8, (255, 0, 0), SurfaceKind::Vehicle);
            }
            renderer.resolve();
            let cell = renderer.cells[12 * 50 + 25];
            assert!(cell.color.0 > 0 && cell.color.1 == 0 && cell.color.2 == 0);
            assert!((cell.depth - 3.0).abs() < 0.01);
        }
    }

    #[test]
    fn resize_discards_old_samples_and_resolves_new_dimensions() {
        let mut renderer = Renderer::new(50, 25, true);
        let tri = [
            Vertex {
                pos: Vec3::new(-1.0, -1.0, 3.0),
            },
            Vertex {
                pos: Vec3::new(1.0, -1.0, 3.0),
            },
            Vertex {
                pos: Vec3::new(0.0, 1.0, 3.0),
            },
        ];
        renderer.rasterize(tri, 1.0, (240, 200, 120), SurfaceKind::Vehicle);
        renderer.resize(24, 12);
        let mut packed = Vec::new();
        renderer.write_packed_cells(&mut packed);
        assert_eq!(packed.len(), 24 * 12);
        assert!(packed.iter().all(|p| (*p & 0xff) == b' ' as u32));
        renderer.rasterize(tri, 1.0, (240, 200, 120), SurfaceKind::Vehicle);
        renderer.write_packed_cells(&mut packed);
        assert!(packed.iter().any(|p| (*p & 0xff) != b' ' as u32));
        renderer.resize(50, 25);
        renderer.write_packed_cells(&mut packed);
        assert_eq!(packed.len(), 50 * 25);
        assert!(packed.iter().all(|p| (*p & 0xff) == b' ' as u32));
    }

    #[test]
    fn lighting_modulates_rgb_as_well_as_ascii() {
        let mut renderer = Renderer::new(50, 25, true);
        let tri = [
            Vertex {
                pos: Vec3::new(-1.0, -1.0, 3.0),
            },
            Vertex {
                pos: Vec3::new(1.0, -1.0, 3.0),
            },
            Vertex {
                pos: Vec3::new(0.0, 1.0, 3.0),
            },
        ];
        renderer.rasterize(tri, 0.2, (200, 100, 50), SurfaceKind::Generic);
        renderer.resolve();
        let dim = renderer.cells[12 * 50 + 25];
        renderer.clear();
        renderer.rasterize(tri, 0.9, (200, 100, 50), SurfaceKind::Generic);
        renderer.resolve();
        let lit = renderer.cells[12 * 50 + 25];
        assert!(dim.color.0 < lit.color.0 && lit.color.0 <= 200);
        assert!(dim.color.1 < lit.color.1 && dim.color.2 < lit.color.2);
    }

    #[test]
    fn distant_marking_between_all_four_samples_remains_lightly_visible() {
        let mut renderer = Renderer::new(50, 25, true);
        let a = Vertex {
            pos: Vec3::new(0.4, -10.0, 50.0),
        };
        let b = Vertex {
            pos: Vec3::new(0.56, -10.0, 50.0),
        };
        let c = Vertex {
            pos: Vec3::new(0.56, 10.0, 50.0),
        };
        let d = Vertex {
            pos: Vec3::new(0.4, 10.0, 50.0),
        };
        for shift in [0.0, 0.1, 0.2, 0.3] {
            renderer.clear();
            let shifted = |v: Vertex| Vertex {
                pos: v.pos + Vec3::X * shift,
            };
            renderer.rasterize(
                [shifted(a), shifted(b), shifted(c)],
                1.0,
                (245, 225, 145),
                SurfaceKind::RoadMarking,
            );
            renderer.rasterize(
                [shifted(a), shifted(c), shifted(d)],
                1.0,
                (245, 225, 145),
                SurfaceKind::RoadMarking,
            );
            renderer.resolve();
            let cell = renderer.cells[12 * 50 + 25];
            assert!(cell.ch != ' ' && cell.ch != '@', "missing at shift {shift}");
            assert!((cell.depth - 50.0).abs() < 0.01);
        }
        renderer.rasterize(
            [
                Vertex {
                    pos: Vec3::new(-1.0, -1.0, 3.0),
                },
                Vertex {
                    pos: Vec3::new(1.0, -1.0, 3.0),
                },
                Vertex {
                    pos: Vec3::new(0.0, 1.0, 3.0),
                },
            ],
            0.8,
            (255, 0, 0),
            SurfaceKind::Vehicle,
        );
        renderer.resolve();
        let occluded = renderer.cells[12 * 50 + 25];
        assert!(occluded.color.0 > 0 && occluded.color.1 == 0 && occluded.color.2 == 0);
        assert!((occluded.depth - 3.0).abs() < 0.01);
    }
}
