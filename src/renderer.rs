pub use crate::glyph::GlyphMode;
use crate::glyph::{CellRole, GlyphInput, choose};
use crate::mesh::{Mesh, RoadRibbon, SurfaceKind};
use glam::{Vec2, Vec3};
use std::fmt::Write as _;

const NEAR: f32 = 0.12;
const FAR: f32 = 170.0;
const SAMPLE_OFFSETS: [(f32, f32); 4] = [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)];
const PALETTE: &[u8] = b" .:-=+*#%@";

struct RibbonTuning {
    aa_radius: f32,
    thin_aa_radius: f32,
    thin_width_start: f32,
    thin_width_end: f32,
    assisted_width: f32,
    width_softness: f32,
    coverage_width: f32,
    width_contribution: f32,
    low_boost: f32,
    low_near_factor: f32,
    low_softness: f32,
    low_start: f32,
    low_end: f32,
    assist_start: f32,
    assist_full: f32,
    fade_start: f32,
    fade_end: f32,
    paint_priority: f32,
    road_blend_gain: f32,
}

const RIBBON: RibbonTuning = RibbonTuning {
    aa_radius: 0.45,
    thin_aa_radius: std::f32::consts::FRAC_1_SQRT_2,
    thin_width_start: 0.15,
    thin_width_end: 0.55,
    assisted_width: 0.4,
    width_softness: 0.1,
    coverage_width: 0.8,
    width_contribution: 0.45,
    low_boost: 0.05,
    low_near_factor: 0.4,
    low_softness: 0.015,
    low_start: 0.06,
    low_end: 0.12,
    assist_start: 0.0,
    assist_full: 20.0,
    fade_start: 95.0,
    fade_end: FAR,
    paint_priority: 36.0,
    road_blend_gain: 10.0,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DebugView {
    #[default]
    Normal,
    Coverage,
    EffectiveCoverage,
    Width,
    Fade,
    RoadMarkings,
}

/// Geometric and depth-resolved perceptual paint signals for a cell.
/// Query after a normal or effective-coverage resolve; raw diagnostic modes
/// intentionally leave effective coverage at zero.
#[derive(Clone, Copy, Debug)]
pub struct RoadMarkingCell {
    pub true_coverage: f32,
    pub effective_coverage: f32,
    pub projected_width: f32,
    pub screen_center: Vec2,
    pub screen_direction: Vec2,
    pub depth: f32,
    pub color: (u8, u8, u8),
    pub brightness: f32,
    pub surface: SurfaceKind,
}

// Keep true AA area separate from the assisted width signal. The latter may
// affect appearance only where the unassisted ribbon actually covers the cell.
#[derive(Clone, Copy)]
struct MarkingCoverage {
    true_coverage: f32,
    assisted_coverage: f32,
    effective_coverage: f32,
    projected_width: f32,
    center: Vec2,
    direction: Vec2,
    inv_depth: f32,
    color: (u8, u8, u8),
    brightness: f32,
}

impl Default for MarkingCoverage {
    fn default() -> Self {
        Self {
            true_coverage: 0.0,
            assisted_coverage: 0.0,
            effective_coverage: 0.0,
            projected_width: 0.0,
            center: Vec2::ZERO,
            direction: Vec2::ZERO,
            inv_depth: 0.0,
            color: (0, 0, 0),
            brightness: 0.0,
        }
    }
}

impl MarkingCoverage {
    fn add(
        &mut self,
        center: Vec2,
        direction: Vec2,
        projected_width: f32,
        true_coverage: f32,
        assisted_coverage: f32,
        inv_depth: f32,
        color: (u8, u8, u8),
        brightness: f32,
    ) {
        if self.true_coverage > 0.0 && self.color != color {
            if inv_depth <= self.inv_depth {
                return;
            }
            *self = Self::default();
        }
        let total = self.true_coverage + true_coverage;
        self.center = (self.center * self.true_coverage + center * true_coverage) / total;
        self.direction = (self.direction * self.true_coverage + direction * true_coverage) / total;
        self.projected_width =
            (self.projected_width * self.true_coverage + projected_width * true_coverage) / total;
        self.inv_depth = (self.inv_depth * self.true_coverage + inv_depth * true_coverage) / total;
        self.brightness =
            (self.brightness * self.true_coverage + brightness * true_coverage) / total;
        self.true_coverage = total;
        self.assisted_coverage += assisted_coverage;
        self.color = color;
    }

    fn depth(self) -> f32 {
        self.inv_depth.recip()
    }
}

fn smoothstep(start: f32, end: f32, value: f32) -> f32 {
    let t = ((value - start) / (end - start)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// Endpoint-only optical width; never replaces the physical projected width.
fn ribbon_visual_width(width: f32, depth: f32) -> f32 {
    let assistance = RIBBON.assisted_width
        * (width / (width + RIBBON.width_softness)).sqrt()
        * (1.0 - smoothstep(0.25, 0.7, width))
        * smoothstep(RIBBON.assist_start, RIBBON.assist_full, depth);
    width + assistance
}

fn marking_fade(depth: f32) -> f32 {
    1.0 - smoothstep(RIBBON.fade_start, RIBBON.fade_end, depth)
}

// A cell's half-diagonal exceeds its center-sample AA radius. Widen only
// the antialiasing support of thin projected ribbons, not their world width
// or maximum opacity, so a line near a cell corner stays faintly represented.
fn ribbon_aa_radius(width: f32) -> f32 {
    RIBBON.aa_radius
        + (RIBBON.thin_aa_radius - RIBBON.aa_radius)
            * (1.0 - smoothstep(RIBBON.thin_width_start, RIBBON.thin_width_end, width))
}

fn ribbon_aa_coverage(distance: f32, width: f32, aa_radius: f32) -> f32 {
    let half = width * 0.5;
    let inner = half - aa_radius;
    let outer = half + aa_radius;
    let edge = if inner > 0.0 && distance <= inner {
        1.0
    } else {
        smoothstep(outer, inner, distance)
    };
    edge * (width / RIBBON.coverage_width).min(1.0)
}

fn marking_visibility(
    true_coverage: f32,
    assisted_coverage: f32,
    projected_width: f32,
    depth: f32,
) -> f32 {
    if true_coverage <= 0.0 {
        return 0.0; // Optical width must never create a mark outside real AA support.
    }
    let true_coverage = true_coverage.min(1.0);
    let optical = true_coverage
        + (assisted_coverage.min(1.0) - true_coverage).max(0.0) * RIBBON.width_contribution;
    let low_coverage = true_coverage
        + (1.0 - true_coverage)
            * RIBBON.low_boost
            * (true_coverage / (true_coverage + RIBBON.low_softness)).sqrt()
            * (1.0 - smoothstep(RIBBON.low_start, RIBBON.low_end, true_coverage))
            * (1.0
                - smoothstep(
                    RIBBON.thin_width_start,
                    RIBBON.thin_width_end,
                    projected_width,
                ))
            * (RIBBON.low_near_factor
                + (1.0 - RIBBON.low_near_factor) * smoothstep(6.0, 35.0, depth));
    optical.max(low_coverage).min(1.0) * marking_fade(depth)
}

fn shade(sample: Sample, coverage: f32) -> (f32, (u8, u8, u8)) {
    let lighting = sample.brightness.clamp(0.0, 1.0);
    // Paint has no fixed glyph floor; other important surfaces retain theirs.
    let intensity = if sample.surface == SurfaceKind::RoadMarking {
        0.65 * coverage.sqrt() * (0.45 + 0.55 * lighting)
    } else {
        (coverage * (0.12 + 0.40 * lighting)).max(if sample.surface.important() {
            0.17
        } else {
            0.0
        })
    };
    let color_scale = (0.38 + 0.62 * lighting) * (0.72 + 0.28 * coverage);
    let lit = |channel: u8| (channel as f32 * color_scale).round().clamp(0.0, 255.0) as u8;
    (
        intensity,
        (
            lit(sample.color.0),
            lit(sample.color.1),
            lit(sample.color.2),
        ),
    )
}

fn resolved_cell(intensity: f32, depth: f32, color: (u8, u8, u8)) -> Cell {
    let index = (intensity * (PALETTE.len() - 1) as f32).round() as usize;
    Cell {
        ch: PALETTE[index] as char,
        depth,
        color,
    }
}

fn debug_marking(strength: f32, depth: f32) -> Cell {
    if strength <= 0.0 {
        return Cell::default();
    }
    let level = strength.min(1.0).sqrt();
    let gray = (level * 255.0).round().max(48.0) as u8;
    Cell {
        ch: PALETTE[(level * 9.0).round().max(1.0) as usize] as char,
        depth,
        color: (gray, gray, gray),
    }
}

// A directional 2x2 mask is only a stroke when nearby visible coverage stays
// on that same one-cell-wide axis. Area beside the axis identifies a filled
// silhouette, including slanted faces without any fully covered cell.
fn mesh_cell_role(
    all_samples: &[Sample],
    width: usize,
    index: usize,
    count: usize,
    sample: Sample,
    coverage: f32,
) -> CellRole {
    if count == 1 || coverage >= 0.72 {
        return CellRole::Filled;
    }
    let center = &all_samples[index * count..(index + 1) * count];
    let mask = center.iter().enumerate().fold(0u8, |mask, (i, s)| {
        if s.surface == sample.surface
            && s.color == sample.color
            && s.depth.is_finite()
            && s.depth <= sample.depth * 1.08
            && sample.depth <= s.depth * 1.08
            && s.coverage > 0.5
        {
            mask | (1 << i)
        } else {
            mask
        }
    });
    let axis = match mask {
        0b0011 | 0b1100 => 0,
        0b0101 | 0b1010 => 1,
        0b1001 => 2,
        0b0110 => 3,
        _ => return CellRole::SilhouetteBoundary,
    };
    let x = index % width;
    let y = index / width;
    let height = all_samples.len() / count / width;
    let mut axis_mass = 0.0;
    let mut cross_axis_mass = 0.0;
    for dy in -1isize..=1 {
        for dx in -1isize..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = x as isize + dx;
            let ny = y as isize + dy;
            if nx < 0 || nx >= width as isize || ny < 0 || ny >= height as isize {
                continue;
            }
            let cell = (ny as usize * width + nx as usize) * count;
            let neighbors = &all_samples[cell..cell + count];
            let nearest = neighbors.iter().fold(f32::INFINITY, |z, s| z.min(s.depth));
            let mass = neighbors
                .iter()
                .filter(|s| {
                    s.surface == sample.surface
                        && s.depth.is_finite()
                        && s.depth <= nearest * 1.08
                        && s.depth <= sample.depth * 1.08
                        && sample.depth <= s.depth * 1.08
                })
                .map(|s| s.coverage)
                .sum::<f32>()
                / count as f32;
            let on_axis = match axis {
                0 => dy == 0,
                1 => dx == 0,
                2 => dx == dy,
                _ => dx == -dy,
            };
            if on_axis {
                axis_mass += mass;
            } else {
                cross_axis_mass += mass;
            }
        }
    }
    if cross_axis_mass >= 0.35 && (cross_axis_mass >= axis_mass * 0.2 || cross_axis_mass >= 0.75) {
        CellRole::SilhouetteBoundary
    } else {
        CellRole::ThinStructure
    }
}

fn shape_cell(
    mode: GlyphMode,
    history: &mut GlyphHistory,
    samples: &[Sample],
    marking: &MarkingCoverage,
    resolved: (Sample, f32, CellRole),
    shaded: (f32, (u8, u8, u8)),
    road_blend: bool,
) -> Cell {
    let (sample, coverage, role) = resolved;
    let (intensity, mut color) = shaded;
    if mode == GlyphMode::Density {
        return resolved_cell(intensity, sample.depth, color);
    }
    let paint = sample.surface == SurfaceKind::RoadMarking;
    if paint && !road_blend {
        // Without asphalt beneath it, coverage must fade RGB as well as ink.
        let strength = (intensity / 0.24).clamp(0.0, 1.0);
        color = (
            (color.0 as f32 * strength).round() as u8,
            (color.1 as f32 * strength).round() as u8,
            (color.2 as f32 * strength).round() as u8,
        );
    }
    if !paint || road_blend {
        // Broad distant surfaces retain their ink; RGB, not punctuation,
        // carries their distance/contrast falloff.
        let fade = 1.0 - 0.48 * smoothstep(45.0, FAR, sample.depth);
        color = (
            (color.0 as f32 * fade).round() as u8,
            (color.1 as f32 * fade).round() as u8,
            (color.2 as f32 * fade).round() as u8,
        );
    }
    let mut next = GlyphHistory {
        ch: ' ',
        depth: sample.depth,
        color,
        brightness: sample.brightness,
        coverage,
        surface: sample.surface,
    };
    let shape = if paint {
        [0.0; 4]
    } else if matches!(sample.surface, SurfaceKind::Road | SurfaceKind::Ground)
        || samples.len() == 1
    {
        [coverage; 4]
    } else {
        std::array::from_fn(|i| {
            let s = samples[i];
            if s.surface == sample.surface
                && s.color == sample.color
                && s.depth.is_finite()
                && s.depth <= sample.depth * 1.08
                && sample.depth <= s.depth * 1.08
            {
                s.coverage
            } else {
                0.0
            }
        })
    };
    let previous = history.compatible(next).then_some(history.ch);
    next.ch = choose(
        GlyphInput {
            surface: sample.surface,
            coverage,
            true_coverage: if paint {
                marking.true_coverage
            } else {
                coverage
            },
            projected_width: if paint { marking.projected_width } else { 0.0 },
            intensity,
            shape,
            direction: if paint {
                marking.direction.normalize_or_zero()
            } else {
                Vec2::ZERO
            },
            center_y: if paint { marking.center.y } else { 0.5 },
            role,
        },
        previous,
    );
    *history = next;
    Cell {
        ch: next.ch,
        depth: sample.depth,
        color,
    }
}

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
struct GlyphHistory {
    ch: char,
    depth: f32,
    color: (u8, u8, u8),
    brightness: f32,
    coverage: f32,
    surface: SurfaceKind,
}

impl Default for GlyphHistory {
    fn default() -> Self {
        Self {
            ch: ' ',
            depth: f32::INFINITY,
            color: (0, 0, 0),
            brightness: 0.0,
            coverage: 0.0,
            surface: SurfaceKind::Generic,
        }
    }
}

impl GlyphHistory {
    fn compatible(self, next: Self) -> bool {
        self.ch != ' '
            && self.surface == next.surface
            && self.depth.is_finite()
            && (self.depth - next.depth).abs() <= next.depth * 0.08
            && (self.brightness - next.brightness).abs() < 0.24
            && (self.coverage - next.coverage).abs() < (next.coverage * 0.5).max(0.15)
            && [
                self.color.0.abs_diff(next.color.0),
                self.color.1.abs_diff(next.color.1),
                self.color.2.abs_diff(next.color.2),
            ]
            .into_iter()
            .max()
            .unwrap()
                < 60
    }
}

#[derive(Clone, Copy)]
struct Vertex {
    pos: Vec3,
}

// Projected paint kept as a continuous width/depth/direction primitive.
#[derive(Clone, Copy)]
struct ProjectedRibbon {
    start: Vec2,
    end: Vec2,
    width_start: f32,
    width_end: f32,
    inv_z_start: f32,
    inv_z_end: f32,
    direction: Vec2,
    inv_length_squared: f32,
    color: (u8, u8, u8),
    brightness: f32,
}

pub struct Renderer {
    pub width: usize,
    pub height: usize,
    cells: Vec<Cell>,
    samples: Vec<Sample>,
    markings: Vec<MarkingCoverage>,
    glyph_history: Vec<GlyphHistory>,
    glyph_mode: GlyphMode,
    last_camera: Option<Camera>,
    frame_camera_seen: bool,
    color: bool,
    cell_aspect: f32,
    aa: AaMode,
    debug_view: DebugView,
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
            markings: vec![MarkingCoverage::default(); width * height],
            glyph_history: vec![GlyphHistory::default(); width * height],
            glyph_mode: GlyphMode::Shape,
            last_camera: None,
            frame_camera_seen: false,
            color,
            cell_aspect,
            aa,
            debug_view: DebugView::Normal,
        }
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
        self.cells.resize(width * height, Cell::default());
        self.samples
            .resize(width * height * self.aa.count(), Sample::default());
        self.markings
            .resize(width * height, MarkingCoverage::default());
        self.glyph_history
            .resize(width * height, GlyphHistory::default());
        self.clear();
        self.reset_glyph_history();
    }

    pub fn clear(&mut self) {
        self.cells.fill(Cell::default());
        self.samples.fill(Sample::default());
        self.markings.fill(MarkingCoverage::default());
        self.frame_camera_seen = false;
    }

    pub fn set_debug_view(&mut self, view: DebugView) {
        self.debug_view = view;
    }

    pub fn set_glyph_mode(&mut self, mode: GlyphMode) {
        if self.glyph_mode != mode {
            self.glyph_mode = mode;
            self.reset_glyph_history();
        }
    }

    pub fn reset_glyph_history(&mut self) {
        self.glyph_history.fill(GlyphHistory::default());
        self.last_camera = None;
    }

    fn observe_camera(&mut self, camera: Camera) {
        if !self.frame_camera_seen {
            if self.last_camera.is_some_and(|previous| {
                previous.position.distance_squared(camera.position) > 9.0
                    || (previous.target - previous.position)
                        .normalize_or_zero()
                        .dot((camera.target - camera.position).normalize_or_zero())
                        < 0.85
            }) {
                self.reset_glyph_history();
            }
            self.last_camera = Some(camera);
            self.frame_camera_seen = true;
        }
    }

    /// Paint data after the last normal or effective-coverage resolve.
    /// Depth occlusion or fade zeroes effective, never geometric, coverage.
    pub fn road_marking_cell(&self, x: usize, y: usize) -> Option<RoadMarkingCell> {
        let marking = self
            .markings
            .get(y.checked_mul(self.width)?.checked_add(x)?)?;
        if x >= self.width || marking.true_coverage <= 0.0 {
            return None;
        }
        Some(RoadMarkingCell {
            true_coverage: marking.true_coverage.min(1.0),
            effective_coverage: marking.effective_coverage,
            projected_width: marking.projected_width,
            screen_center: marking.center,
            screen_direction: marking.direction.normalize_or_zero(),
            depth: marking.depth(),
            color: marking.color,
            brightness: marking.brightness,
            surface: SurfaceKind::RoadMarking,
        })
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
        self.observe_camera(camera);
        let forward = (camera.target - camera.position).normalize_or_zero();
        let right = Vec3::Y.cross(forward).normalize_or_zero();
        let up = forward.cross(right);
        let light = Vec3::new(-0.35, 0.85, -0.4).normalize();
        for triangle in &mesh.triangles {
            if triangle.surface == SurfaceKind::RoadMarking {
                continue; // Paint is exclusively drawn from RoadRibbon.
            }
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

    /// Road paint uses projected world-space centerlines, never point-sampled triangles.
    pub fn draw_ribbons(&mut self, ribbons: &[RoadRibbon], camera: Camera) {
        if self.width == 0 || self.height == 0 {
            return;
        }
        self.observe_camera(camera);
        let forward = (camera.target - camera.position).normalize_or_zero();
        let right = Vec3::Y.cross(forward).normalize_or_zero();
        let up = forward.cross(right);
        let scale = self.height as f32 / (2.0 * (70.0_f32.to_radians() * 0.5).tan());
        let horizontal_scale = scale / self.cell_aspect;
        let screen_center = Vec2::new(self.width as f32 * 0.5, self.height as f32 * 0.5);
        let light = Vec3::new(-0.35, 0.85, -0.4).normalize();
        let to_view = |p: Vec3| {
            let d = p - camera.position;
            Vec3::new(d.dot(right), d.dot(up), d.dot(forward))
        };
        let project = |p: Vec3| {
            let inv_z = p.z.recip();
            screen_center + Vec2::new(p.x * inv_z * horizontal_scale, -p.y * inv_z * scale)
        };
        for ribbon in ribbons {
            if ribbon.width <= 0.0 {
                continue;
            }
            let axis = ribbon.end - ribbon.start;
            let lateral = Vec3::new(-axis.z, 0.0, axis.x).normalize_or_zero();
            if lateral == Vec3::ZERO {
                continue;
            }
            let lateral = lateral * (ribbon.width * 0.5);
            let lateral_view = Vec3::new(lateral.dot(right), lateral.dot(up), lateral.dot(forward));
            let mut a = to_view(ribbon.start);
            let mut b = to_view(ribbon.end);
            if (a.z < NEAR && b.z < NEAR) || (a.z > FAR && b.z > FAR) {
                continue;
            }
            // Clip before projection: a crossing endpoint never creates a
            // huge or negative screen-space coordinate.
            if a.z < NEAR {
                a = a.lerp(b, (NEAR - a.z) / (b.z - a.z));
            } else if b.z < NEAR {
                b = b.lerp(a, (NEAR - b.z) / (a.z - b.z));
            }
            if a.z > FAR {
                a = a.lerp(b, (FAR - a.z) / (b.z - a.z));
            } else if b.z > FAR {
                b = b.lerp(a, (FAR - b.z) / (a.z - b.z));
            }
            let screen_a = project(a);
            let screen_b = project(b);
            let ab = screen_b - screen_a;
            let length_squared = ab.length_squared();
            let (direction, inv_length_squared) = if length_squared > 1e-8 {
                let mut direction = ab / length_squared.sqrt();
                // Undirected line: stable orientation when a segment is reversed.
                if direction.x < 0.0 || (direction.x == 0.0 && direction.y < 0.0) {
                    direction = -direction;
                }
                (direction, length_squared.recip())
            } else {
                (Vec2::X, 0.0)
            };
            let projected_half_width = |p: Vec3| {
                let inv_z = p.z.recip();
                let displacement = Vec2::new(
                    (lateral_view.x - p.x * lateral_view.z * inv_z) * inv_z * horizontal_scale,
                    -(lateral_view.y - p.y * lateral_view.z * inv_z) * inv_z * scale,
                );
                if inv_length_squared > 0.0 {
                    displacement.perp_dot(direction).abs()
                } else {
                    displacement.length()
                }
            };
            self.rasterize_ribbon(ProjectedRibbon {
                start: screen_a,
                end: screen_b,
                width_start: 2.0 * projected_half_width(a),
                width_end: 2.0 * projected_half_width(b),
                inv_z_start: a.z.recip(),
                inv_z_end: b.z.recip(),
                direction,
                inv_length_squared,
                color: ribbon.color,
                brightness: (0.22 + 0.78 * light.y) * ribbon.shade,
            });
        }
    }
    fn rasterize_ribbon(&mut self, ribbon: ProjectedRibbon) {
        let width_a = ribbon_visual_width(ribbon.width_start, ribbon.inv_z_start.recip());
        let width_b = ribbon_visual_width(ribbon.width_end, ribbon.inv_z_end.recip());
        let aa_a = ribbon_aa_radius(ribbon.width_start);
        let aa_b = ribbon_aa_radius(ribbon.width_end);
        let radius = ribbon.width_start.max(ribbon.width_end) * 0.5 + aa_a.max(aa_b);
        let min = ribbon.start.min(ribbon.end) - Vec2::splat(radius + 0.5);
        let max = ribbon.start.max(ribbon.end) + Vec2::splat(radius - 0.5);
        if max.x < 0.0 || max.y < 0.0 || min.x >= self.width as f32 || min.y >= self.height as f32 {
            return;
        }
        let min_x = min.x.ceil().max(0.0) as usize;
        let min_y = min.y.ceil().max(0.0) as usize;
        let max_x = max.x.floor().min((self.width - 1) as f32) as usize;
        let max_y = max.y.floor().min((self.height - 1) as f32) as usize;
        if min_x > max_x || min_y > max_y {
            return;
        }
        let ab = ribbon.end - ribbon.start;
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let t = ((p - ribbon.start).dot(ab) * ribbon.inv_length_squared).clamp(0.0, 1.0);
                let closest = ribbon.start + ab * t;
                let distance_squared = (p - closest).length_squared();
                let projected_width =
                    ribbon.width_start + (ribbon.width_end - ribbon.width_start) * t;
                let aa = aa_a + (aa_b - aa_a) * t;
                let outer = projected_width * 0.5 + aa;
                if distance_squared >= outer * outer {
                    continue;
                }
                let distance = distance_squared.sqrt();
                let true_coverage = ribbon_aa_coverage(distance, projected_width, aa);
                if true_coverage <= 0.0 {
                    continue;
                }
                let assisted_width = width_a + (width_b - width_a) * t;
                let assisted_coverage = if assisted_width > projected_width {
                    ribbon_aa_coverage(distance, assisted_width, aa)
                } else {
                    true_coverage
                };
                let inv_depth = ribbon.inv_z_start + (ribbon.inv_z_end - ribbon.inv_z_start) * t;
                self.markings[y * self.width + x].add(
                    (closest - Vec2::new(x as f32, y as f32)).clamp(Vec2::ZERO, Vec2::ONE),
                    ribbon.direction,
                    projected_width,
                    true_coverage,
                    assisted_coverage,
                    inv_depth,
                    ribbon.color,
                    ribbon.brightness,
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
                for (sample_index, &offset) in SAMPLE_OFFSETS.iter().enumerate().take(count) {
                    let (dx, dy) = if count == 1 { (0.5, 0.5) } else { offset };
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
            }
        }
    }

    fn resolve(&mut self) {
        let count = self.aa.count();
        let all_samples = &self.samples;
        for (index, (((cell, samples), marking), history)) in self
            .cells
            .iter_mut()
            .zip(all_samples.chunks_exact(count))
            .zip(&mut self.markings)
            .zip(&mut self.glyph_history)
            .enumerate()
        {
            marking.effective_coverage = 0.0;
            if cell.depth < 0.0 {
                continue; // HUD overlay
            }
            let marking_depth = if marking.true_coverage > 0.0 {
                marking.depth()
            } else {
                f32::INFINITY
            };
            match self.debug_view {
                DebugView::Coverage => {
                    *cell = debug_marking(marking.true_coverage, marking_depth);
                    continue;
                }
                DebugView::Width => {
                    *cell = debug_marking(
                        if marking.true_coverage > 0.0 {
                            marking.projected_width / RIBBON.coverage_width
                        } else {
                            0.0
                        },
                        marking_depth,
                    );
                    continue;
                }
                DebugView::Fade => {
                    *cell = debug_marking(
                        if marking.true_coverage > 0.0 {
                            marking_fade(marking_depth)
                        } else {
                            0.0
                        },
                        marking_depth,
                    );
                    continue;
                }
                _ => {}
            }

            let nearest_depth = samples.iter().fold(marking_depth, |z, s| z.min(s.depth));
            // Foreground samples conservatively occlude covered ribbon cells.
            // Coplanar road/ground can still compete by coverage.
            let visible_marking = marking.true_coverage > 0.0
                && marking_depth <= nearest_depth * 1.08
                && !samples.iter().any(|s| {
                    s.depth < marking_depth
                        && !matches!(s.surface, SurfaceKind::Road | SurfaceKind::Ground)
                });
            let mut winner: Option<(Sample, f32)> = None;
            let mut best_score = 0.0;
            for candidate in samples.iter().filter(|s| s.depth.is_finite()) {
                if candidate.depth > nearest_depth * 1.08 {
                    continue;
                }
                let coverage = samples
                    .iter()
                    .filter(|s| {
                        s.surface == candidate.surface
                            && s.color == candidate.color
                            && s.depth.is_finite()
                            && s.depth <= candidate.depth * 1.08
                            && candidate.depth <= s.depth * 1.08
                    })
                    .map(|s| s.coverage)
                    .sum::<f32>()
                    / count as f32;
                let score = coverage
                    * if candidate.surface.important() {
                        3.2
                    } else {
                        1.0
                    };
                if score > best_score
                    || (score == best_score
                        && winner.is_none_or(|(w, _)| candidate.depth < w.depth))
                {
                    best_score = score;
                    winner = Some((*candidate, coverage));
                }
            }
            let road_background = winner.filter(|(sample, _)| {
                matches!(sample.surface, SurfaceKind::Road | SurfaceKind::Ground)
            });
            let effective = if visible_marking {
                marking_visibility(
                    marking.true_coverage,
                    marking.assisted_coverage,
                    marking.projected_width,
                    marking_depth,
                )
            } else {
                0.0
            };
            marking.effective_coverage = effective;
            if self.debug_view == DebugView::EffectiveCoverage {
                *cell = debug_marking(effective, marking_depth);
                continue;
            }
            let paint_present = effective > 0.0;
            if paint_present && effective * RIBBON.paint_priority > best_score {
                winner = Some((
                    Sample {
                        depth: marking_depth,
                        color: marking.color,
                        brightness: marking.brightness,
                        surface: SurfaceKind::RoadMarking,
                        coverage: effective,
                    },
                    effective,
                ));
            }
            if self.debug_view == DebugView::RoadMarkings {
                *cell = if marking.true_coverage > 0.0 {
                    let contributes = paint_present
                        && (road_background.is_some()
                            || winner.is_some_and(|(sample, _)| {
                                sample.surface == SurfaceKind::RoadMarking
                            }));
                    Cell {
                        ch: if contributes { '#' } else { '.' },
                        depth: marking_depth,
                        color: if contributes {
                            (0, 255, 0)
                        } else if visible_marking {
                            (255, 255, 0) // distance fade or coverage lost during scoring
                        } else {
                            (255, 0, 0) // depth rejection
                        },
                    }
                } else {
                    Cell::default()
                };
                continue;
            }
            if let Some((road, road_coverage)) = road_background.filter(|_| paint_present) {
                // Paint and road are two layers of the same surface. Blend even
                // when paint's score falls below road's: otherwise a distant
                // line switches abruptly from yellow to bare road at that tie.
                let (road_intensity, road_color) = shade(road, road_coverage);
                let paint = Sample {
                    depth: marking_depth,
                    color: marking.color,
                    brightness: marking.brightness,
                    surface: SurfaceKind::RoadMarking,
                    coverage: effective,
                };
                let (paint_intensity, paint_color) = shade(paint, effective);
                let opacity = 1.0 - (-RIBBON.road_blend_gain * effective).exp();
                let intensity = road_intensity
                    + opacity * (paint_intensity.max(road_intensity + 0.15) - road_intensity);
                let mix = |base: u8, paint: u8| {
                    (base as f32 + (paint as f32 - base as f32) * opacity).round() as u8
                };
                let color = (
                    mix(road_color.0, paint_color.0),
                    mix(road_color.1, paint_color.1),
                    mix(road_color.2, paint_color.2),
                );
                *cell = shape_cell(
                    self.glyph_mode,
                    history,
                    samples,
                    marking,
                    (paint, effective, CellRole::Filled),
                    (intensity, color),
                    true,
                );
            } else if let Some((sample, coverage)) = winner {
                let (intensity, color) = shade(sample, coverage);
                let role = if self.glyph_mode == GlyphMode::Shape
                    && !matches!(
                        sample.surface,
                        SurfaceKind::Road | SurfaceKind::Ground | SurfaceKind::RoadMarking
                    ) {
                    mesh_cell_role(all_samples, self.width, index, count, sample, coverage)
                } else {
                    CellRole::Filled
                };
                *cell = shape_cell(
                    self.glyph_mode,
                    history,
                    samples,
                    marking,
                    (sample, coverage, role),
                    (intensity, color),
                    false,
                );
            } else {
                *history = GlyphHistory::default();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::Mesh;
    use crate::{car::Car, track::Track};
    use glam::{Mat4, Vec3};
    const CELL_INDEX: usize = 12 * 50 + 25;
    const PAINT: (u8, u8, u8) = (245, 225, 145);

    fn screen_vertex(p: Vec2, depth: f32) -> Vertex {
        let scale = 25.0 / (2.0 * (70.0_f32.to_radians() * 0.5).tan());
        Vertex {
            pos: Vec3::new(
                (p.x - 25.0) * depth * 0.5 / scale,
                (12.5 - p.y) * depth / scale,
                depth,
            ),
        }
    }

    fn screen_ribbon(
        start: Vec2,
        end: Vec2,
        widths: [f32; 2],
        depths: [f32; 2],
    ) -> ProjectedRibbon {
        let ab = end - start;
        let length_squared = ab.length_squared();
        let mut direction = ab / length_squared.sqrt();
        if direction.x < 0.0 || (direction.x == 0.0 && direction.y < 0.0) {
            direction = -direction;
        }
        ProjectedRibbon {
            start,
            end,
            width_start: widths[0],
            width_end: widths[1],
            inv_z_start: depths[0].recip(),
            inv_z_end: depths[1].recip(),
            direction,
            inv_length_squared: length_squared.recip(),
            color: PAINT,
            brightness: 1.0,
        }
    }

    fn paint_strip(renderer: &mut Renderer, center: Vec2, degrees: f32, width: f32, depth: f32) {
        let angle = degrees.to_radians();
        let along = Vec2::new(angle.cos(), angle.sin()) * 0.7;
        renderer.rasterize_ribbon(screen_ribbon(
            center - along,
            center + along,
            [width; 2],
            [depth; 2],
        ));
    }

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
        renderer.draw_ribbons(&track.ribbons, camera);
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
    fn real_car_top_is_filled_not_an_ascii_cap() {
        let mut renderer = Renderer::new(109, 33, true);
        let car = Car::new();
        let camera = Camera {
            position: Vec3::new(0.0, 3.7, -6.3),
            target: Vec3::new(0.0, 0.8, 3.0),
        };
        renderer.draw_mesh(&car.mesh(), camera);
        renderer.resolve();
        let red_body = |index: usize| {
            let color = renderer.cells[index].color;
            renderer.glyph_history[index].surface == SurfaceKind::Vehicle
                && (color.0 as u16) > (color.1 as u16) * 2
                && (color.0 as u16) > (color.2 as u16) * 2
        };
        let top = (0..renderer.cells.len())
            .filter(|&index| red_body(index))
            .map(|index| index / renderer.width)
            .min()
            .unwrap();
        let cap: String = (top * renderer.width..(top + 1) * renderer.width)
            .filter(|&index| red_body(index))
            .map(|index| renderer.cells[index].ch)
            .collect();
        assert!(cap.len() >= 3, "too few car roof cells: {cap}");
        assert!(cap.chars().all(|ch| "=+*#%@".contains(ch)), "roof: {cap}");
        assert!(
            renderer
                .cells
                .iter()
                .enumerate()
                .filter(|(index, _)| red_body(*index))
                .all(|(_, cell)| !"()[]".contains(cell.ch))
        );
    }

    #[test]
    fn neighbor_fill_disambiguates_a_thin_generic_stroke_from_a_solid_corner() {
        let mut renderer = Renderer::new(50, 25, true);
        for (occupied, line) in [
            ([false, true, true, false], '/'),
            ([true, false, false, true], '\\'),
        ] {
            renderer.clear();
            let part = Sample {
                depth: 5.0,
                color: (200, 140, 90),
                brightness: 0.6,
                coverage: 1.0,
                surface: SurfaceKind::Generic,
            };
            for (s, active) in renderer.samples[CELL_INDEX * 4..CELL_INDEX * 4 + 4]
                .iter_mut()
                .zip(occupied)
            {
                if active {
                    *s = part;
                }
            }
            renderer.resolve();
            assert_eq!(renderer.cells[CELL_INDEX].ch, line);

            // Same partial mask, but a full cell immediately beneath it now
            // proves this is the AA edge of a filled face, not a diagonal line.
            renderer.samples[(CELL_INDEX + 50) * 4..(CELL_INDEX + 51) * 4].fill(part);
            renderer.resolve();
            assert!("=+*#%@".contains(renderer.cells[CELL_INDEX].ch));
            assert_ne!(
                renderer.cells[CELL_INDEX].ch, line,
                "history kept a stroke on a solid"
            );
        }
    }

    fn put_mesh_mask(
        renderer: &mut Renderer,
        x: usize,
        y: usize,
        surface: SurfaceKind,
        mask: [bool; 4],
    ) {
        let start = (y * renderer.width + x) * 4;
        for (slot, active) in renderer.samples[start..start + 4].iter_mut().zip(mask) {
            if active {
                *slot = Sample {
                    depth: 5.0,
                    color: (210, 160, 100),
                    brightness: 0.65,
                    coverage: 1.0,
                    surface,
                };
            }
        }
    }

    #[test]
    fn three_sample_diagonal_boundary_uses_fill_ink() {
        for surface in [
            SurfaceKind::Vehicle,
            SurfaceKind::Guardrail,
            SurfaceKind::Generic,
            SurfaceKind::Checkpoint,
        ] {
            let mut renderer = Renderer::new(50, 25, false);
            put_mesh_mask(&mut renderer, 25, 12, surface, [false, true, true, true]);
            renderer.resolve();
            assert!(
                "=+*#%@".contains(renderer.cells[CELL_INDEX].ch),
                "{surface:?}"
            );
        }
    }

    #[test]
    fn slanted_broad_strip_without_full_cells_is_a_silhouette_for_every_mesh_kind() {
        for surface in [
            SurfaceKind::Vehicle,
            SurfaceKind::Guardrail,
            SurfaceKind::Generic,
            SurfaceKind::Checkpoint,
        ] {
            let mut renderer = Renderer::new(50, 25, false);
            // Two parallel diagonal runs reproduce the guardrail edge: every
            // occupied cell has only two samples, including all neighbors.
            for step in -2isize..=2 {
                let x = (25 + step) as usize;
                let y = (12 - step) as usize;
                put_mesh_mask(&mut renderer, x, y, surface, [false, true, true, false]);
                put_mesh_mask(&mut renderer, x + 1, y, surface, [false, true, true, false]);
            }
            let part = renderer.samples[CELL_INDEX * 4 + 1];
            assert_eq!(
                mesh_cell_role(&renderer.samples, 50, CELL_INDEX, 4, part, 0.5),
                CellRole::SilhouetteBoundary,
                "{surface:?}"
            );
            renderer.resolve();
            for step in -1isize..=1 {
                let index = (12 - step) as usize * 50 + (25 + step) as usize;
                assert!(
                    "=+*#%@".contains(renderer.cells[index].ch),
                    "{surface:?} step {step}: {}",
                    renderer.cells[index].ch
                );
            }
        }
    }

    #[test]
    fn one_cell_strokes_keep_diagonal_horizontal_and_vertical_glyphs() {
        let cases = [
            ([false, true, true, false], (1isize, -1isize), '/'),
            ([true, false, false, true], (1, 1), '\\'),
            ([true, true, false, false], (1, 0), '-'),
            ([true, false, true, false], (0, 1), '|'),
        ];
        for surface in [
            SurfaceKind::Vehicle,
            SurfaceKind::Guardrail,
            SurfaceKind::Generic,
            SurfaceKind::Checkpoint,
        ] {
            for (mask, (dx, dy), expected) in cases {
                let mut renderer = Renderer::new(50, 25, false);
                for offset in -1isize..=1 {
                    put_mesh_mask(
                        &mut renderer,
                        (25 + offset * dx) as usize,
                        (12 + offset * dy) as usize,
                        surface,
                        mask,
                    );
                }
                let part = *renderer.samples[CELL_INDEX * 4..CELL_INDEX * 4 + 4]
                    .iter()
                    .find(|s| s.depth.is_finite())
                    .unwrap();
                assert_eq!(
                    mesh_cell_role(&renderer.samples, 50, CELL_INDEX, 4, part, 0.5),
                    CellRole::ThinStructure
                );
                renderer.resolve();
                assert_eq!(
                    renderer.cells[CELL_INDEX].ch, expected,
                    "{surface:?} {mask:?}"
                );
            }
        }
    }

    #[test]
    fn hidden_neighbor_coverage_does_not_make_a_stroke_look_filled() {
        let mut renderer = Renderer::new(50, 25, false);
        let mask = [false, true, true, false];
        put_mesh_mask(&mut renderer, 25, 12, SurfaceKind::Generic, mask);
        put_mesh_mask(&mut renderer, 26, 12, SurfaceKind::Generic, mask);
        let part = renderer.samples[CELL_INDEX * 4 + 1];
        assert_eq!(
            mesh_cell_role(&renderer.samples, 50, CELL_INDEX, 4, part, 0.5),
            CellRole::SilhouetteBoundary
        );
        renderer.samples[(CELL_INDEX + 1) * 4] = Sample {
            depth: 2.0,
            color: (0, 0, 0),
            brightness: 0.5,
            coverage: 1.0,
            surface: SurfaceKind::Vehicle,
        };
        assert_eq!(
            mesh_cell_role(&renderer.samples, 50, CELL_INDEX, 4, part, 0.5),
            CellRole::ThinStructure
        );
    }

    #[test]
    fn subcell_marking_survives_without_center_coverage() {
        let mut renderer = Renderer::new(50, 25, false);
        paint_strip(&mut renderer, Vec2::new(25.38, 12.5), 90.0, 0.02, 5.0);
        renderer.resolve();
        assert!(renderer.markings[CELL_INDEX].true_coverage > 0.0);
        assert_ne!(renderer.cells[CELL_INDEX].ch, ' ');
        assert!(
            renderer.samples[CELL_INDEX * 4..CELL_INDEX * 4 + 4]
                .iter()
                .all(|s| !s.depth.is_finite())
        );
        let mut off = Renderer::with_options(50, 25, false, 0.5, AaMode::Off);
        paint_strip(&mut off, Vec2::new(25.38, 12.5), 90.0, 0.02, 5.0);
        off.resolve();
        assert_eq!(off.cells[CELL_INDEX].ch, renderer.cells[CELL_INDEX].ch);
    }

    #[test]
    fn nearer_surface_hides_thin_marking_even_when_drawn_first() {
        let mut renderer = Renderer::new(50, 25, true);
        let foreground = [
            screen_vertex(Vec2::new(24.0, 13.5), 3.0),
            screen_vertex(Vec2::new(26.0, 13.5), 3.0),
            screen_vertex(Vec2::new(25.0, 11.5), 3.0),
        ];
        for near_first in [false, true] {
            renderer.clear();
            if near_first {
                renderer.rasterize(foreground, 0.8, (255, 0, 0), SurfaceKind::Vehicle);
            }
            paint_strip(&mut renderer, Vec2::new(25.5, 12.5), 90.0, 0.08, 5.0);
            if !near_first {
                renderer.rasterize(foreground, 0.8, (255, 0, 0), SurfaceKind::Vehicle);
            }
            renderer.resolve();
            let cell = renderer.cells[CELL_INDEX];
            assert!(cell.color.0 > 0 && cell.color.1 == 0 && cell.color.2 == 0);
            assert!((cell.depth - 3.0).abs() < 0.01);
            let paint = renderer.road_marking_cell(25, 12).unwrap();
            assert!(paint.true_coverage > 0.0);
            assert_eq!(paint.effective_coverage, 0.0);
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
    fn lighting_modulates_rgb_for_opaque_geometry() {
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
    fn distant_opaque_faces_keep_ink_and_dim_in_rgb() {
        let mut renderer = Renderer::new(50, 25, true);
        for surface in [SurfaceKind::Generic, SurfaceKind::Road] {
            let mut observed = Vec::new();
            for depth in [5.0, 140.0] {
                renderer.clear();
                let corners = [
                    Vec2::new(25.0, 12.0),
                    Vec2::new(25.0, 13.0),
                    Vec2::new(26.0, 13.0),
                    Vec2::new(26.0, 12.0),
                ]
                .map(|p| screen_vertex(p, depth));
                renderer.rasterize(
                    [corners[0], corners[1], corners[2]],
                    0.7,
                    (180, 160, 130),
                    surface,
                );
                renderer.rasterize(
                    [corners[0], corners[2], corners[3]],
                    0.7,
                    (180, 160, 130),
                    surface,
                );
                renderer.resolve();
                observed.push(renderer.cells[CELL_INDEX]);
            }
            assert_eq!(observed[0].ch, observed[1].ch, "{surface:?}");
            assert!(if surface == SurfaceKind::Road {
                observed[1].ch == ':'
            } else {
                "=+*#%@".contains(observed[1].ch)
            });
            assert!(
                (observed[1].color.0 as u16) * 4 < (observed[0].color.0 as u16) * 3,
                "{surface:?}"
            );
            assert!(observed[1].color.0 > 0);
        }
    }

    #[test]
    fn diagonal_between_msaa_points_retains_ribbon_coverage() {
        let mut renderer = Renderer::new(50, 25, true);
        let angle = 45.0_f32.to_radians();
        let normal = Vec2::new(-angle.sin(), angle.cos());
        let center = Vec2::new(25.5, 12.5) + normal * 0.12;
        for (dx, dy) in SAMPLE_OFFSETS {
            let sample = Vec2::new(25.0 + dx, 12.0 + dy);
            assert!((sample - center).dot(normal).abs() > 0.02);
        }
        paint_strip(&mut renderer, center, 45.0, 0.04, 50.0);
        assert!(
            renderer.samples[CELL_INDEX * 4..CELL_INDEX * 4 + 4]
                .iter()
                .all(|s| !s.depth.is_finite())
        );
        assert!(renderer.markings[CELL_INDEX].true_coverage > 0.0);
        let observed = renderer.markings[CELL_INDEX].direction.normalize();
        assert!(observed.dot(Vec2::new(angle.cos(), angle.sin())) > 0.99);
        assert!(renderer.markings[CELL_INDEX].center.min_element() >= 0.0);
        assert!(renderer.markings[CELL_INDEX].center.max_element() <= 1.0);
        renderer.resolve();
        assert_ne!(renderer.cells[CELL_INDEX].ch, ' ');
        assert!((renderer.cells[CELL_INDEX].depth - 50.0).abs() < 0.01);
        let paint = renderer.road_marking_cell(25, 12).unwrap();
        assert!(paint.true_coverage > 0.0 && paint.true_coverage < 0.04);
        assert!(paint.effective_coverage > paint.true_coverage && paint.effective_coverage < 0.2);
        assert!((paint.projected_width - 0.04).abs() < 1e-5);
        assert!(
            paint
                .screen_direction
                .dot(Vec2::new(angle.cos(), angle.sin()))
                > 0.99
        );
        assert_eq!(paint.surface, SurfaceKind::RoadMarking);
        assert_eq!(paint.color, PAINT);
        assert!(paint.brightness > 0.0);
    }

    #[test]
    fn zero_geometric_support_never_creates_optical_ghosts() {
        let mut renderer = Renderer::new(50, 25, true);
        let optical_width = ribbon_visual_width(0.01, 50.0);
        let aa = ribbon_aa_radius(0.01);
        assert!(ribbon_aa_coverage(0.75, optical_width, aa) > 0.0);
        assert_eq!(ribbon_aa_coverage(0.75, 0.01, aa), 0.0);
        paint_strip(&mut renderer, Vec2::new(25.5, 11.75), 0.0, 0.01, 50.0);
        renderer.resolve();
        assert!(renderer.road_marking_cell(25, 12).is_none());
        assert_eq!(renderer.cells[CELL_INDEX].ch, ' ');
        assert_eq!(marking_visibility(0.0, 0.3, 0.01, 50.0), 0.0);
    }

    #[test]
    fn rotating_thin_paint_keeps_continuous_coverage_and_visibility() {
        let mut renderer = Renderer::new(50, 25, true);
        let mut previous: Option<f32> = None;
        let mut saw_point_hit = false;
        let mut saw_point_miss = false;
        for degrees in [
            0.0_f32, 5.0, 10.0, 15.0, 20.0, 25.0, 30.0, 45.0, 60.0, 75.0, 90.0,
        ] {
            renderer.clear();
            let angle = degrees.to_radians();
            let normal = Vec2::new(-angle.sin(), angle.cos());
            let center = Vec2::new(25.5, 12.5) + normal * 0.12;
            let along = Vec2::new(angle.cos(), angle.sin());
            let hit = SAMPLE_OFFSETS.iter().any(|&(dx, dy)| {
                let delta = Vec2::new(25.0 + dx, 12.0 + dy) - center;
                delta.dot(normal).abs() < 0.02 && delta.dot(along).abs() < 0.7
            });
            saw_point_hit |= hit;
            saw_point_miss |= !hit;
            paint_strip(&mut renderer, center, degrees, 0.04, 45.0);
            let coverage = renderer.markings[CELL_INDEX].true_coverage;
            assert!(coverage > 0.0, "lost coverage at {degrees}°");
            if let Some(previous) = previous {
                assert!(
                    (coverage - previous).abs() < 0.1,
                    "coverage jumped at {degrees}°: {previous} -> {coverage}"
                );
            }
            previous = Some(coverage);
            renderer.resolve();
            assert_ne!(renderer.cells[CELL_INDEX].ch, ' ', "missing at {degrees}°");
        }
        assert!(
            saw_point_hit && saw_point_miss,
            "test must cross the point-hit boundary"
        );
    }

    #[test]
    fn very_thin_ribbon_contributes_to_road_color() {
        let mut renderer = Renderer::new(50, 25, true);
        let corners = [
            Vec2::new(25.0, 12.0),
            Vec2::new(25.0, 13.0),
            Vec2::new(26.0, 13.0),
            Vec2::new(26.0, 12.0),
        ]
        .map(|p| screen_vertex(p, 5.03));
        renderer.rasterize(
            [corners[0], corners[1], corners[2]],
            0.6,
            (87, 91, 96),
            SurfaceKind::Road,
        );
        renderer.rasterize(
            [corners[0], corners[2], corners[3]],
            0.6,
            (87, 91, 96),
            SurfaceKind::Road,
        );
        renderer.resolve();
        let bare_road = renderer.cells[CELL_INDEX].color;
        paint_strip(&mut renderer, Vec2::new(25.105, 12.5), 90.0, 0.005, 5.0);
        let coverage = renderer.markings[CELL_INDEX].true_coverage;
        assert!(coverage > 0.0 && coverage < 0.01, "{coverage}");
        renderer.resolve();
        let cell = renderer.cells[CELL_INDEX];
        assert_ne!(cell.ch, ' ');
        assert!(
            cell.color.0 > bare_road.0 && cell.color.1 > bare_road.1 && cell.color.2 >= bare_road.2,
            "paint made no visible contribution: {:?} -> {:?}",
            bare_road,
            cell.color
        );
        assert!((cell.depth - 5.0).abs() < 0.01);
    }

    #[test]
    fn faint_marking_remains_distinct_over_road_as_view_shifts() {
        let mut renderer = Renderer::new(50, 25, true);
        let corners = [
            Vec2::new(25.0, 12.0),
            Vec2::new(25.0, 13.0),
            Vec2::new(26.0, 13.0),
            Vec2::new(26.0, 12.0),
        ]
        .map(|p| screen_vertex(p, 50.03));
        let mut previous_contrast = None;
        for x in [25.27_f32, 25.29, 25.31, 25.33] {
            renderer.clear();
            renderer.rasterize(
                [corners[0], corners[1], corners[2]],
                0.6,
                (87, 91, 96),
                SurfaceKind::Road,
            );
            renderer.rasterize(
                [corners[0], corners[2], corners[3]],
                0.6,
                (87, 91, 96),
                SurfaceKind::Road,
            );
            renderer.resolve();
            let base = renderer.cells[CELL_INDEX].color;
            paint_strip(&mut renderer, Vec2::new(x, 12.5), 90.0, 0.015, 50.0);
            renderer.resolve();
            let paint = renderer.road_marking_cell(25, 12).unwrap();
            assert!(paint.true_coverage > 0.0 && paint.true_coverage < 0.02);
            assert!(paint.effective_coverage > paint.true_coverage);
            let result = renderer.cells[CELL_INDEX];
            let contrast = result.color.0 as i32 - base.0 as i32;
            assert!(
                contrast >= 2 && result.ch != ' ',
                "{x}: {:?} -> {:?}",
                base,
                result.color
            );
            if let Some(previous) = previous_contrast {
                assert!(
                    contrast >= previous && contrast - previous < 30,
                    "x={x} contrast={contrast} previous={previous} true={} effective={}",
                    paint.true_coverage,
                    paint.effective_coverage
                );
            }
            assert!((result.depth - 50.0).abs() < 0.01);
            previous_contrast = Some(contrast);
        }
    }

    #[test]
    fn perspective_ribbon_depth_and_nearer_foreground_occlude_paint() {
        let mut renderer = Renderer::new(50, 25, true);
        let ribbon = screen_ribbon(
            Vec2::new(25.1, 12.5),
            Vec2::new(25.9, 12.5),
            [0.12, 0.06],
            [4.0, 8.0],
        );
        renderer.rasterize_ribbon(ribbon);
        assert!((renderer.markings[CELL_INDEX].depth() - 5.333333).abs() < 0.01);
        let corners = [
            Vec2::new(25.0, 12.0),
            Vec2::new(25.0, 13.0),
            Vec2::new(26.0, 13.0),
            Vec2::new(26.0, 12.0),
        ]
        .map(|p| screen_vertex(p, 5.0));
        renderer.rasterize(
            [corners[0], corners[1], corners[2]],
            0.8,
            (255, 0, 0),
            SurfaceKind::Vehicle,
        );
        renderer.rasterize(
            [corners[0], corners[2], corners[3]],
            0.8,
            (255, 0, 0),
            SurfaceKind::Vehicle,
        );
        renderer.resolve();
        assert_eq!(renderer.cells[CELL_INDEX].depth, 5.0);
        assert_eq!(renderer.cells[CELL_INDEX].color.1, 0);
    }

    #[test]
    fn perspective_width_changes_gradually_along_the_ribbon() {
        let mut renderer = Renderer::new(50, 25, false);
        let ribbon = screen_ribbon(
            Vec2::new(25.5, 9.5),
            Vec2::new(25.5, 15.5),
            [0.7, 0.1],
            [5.0, 60.0],
        );
        assert!(ribbon_visual_width(0.1, 60.0) > 0.3);
        renderer.rasterize_ribbon(ribbon);
        let near = renderer.markings[10 * 50 + 25];
        let far = renderer.markings[14 * 50 + 25];
        assert!(near.true_coverage > far.true_coverage && far.true_coverage > 0.0);
        assert!(near.depth() < far.depth());
        for y in 10..15 {
            let current = renderer.markings[y * 50 + 25];
            assert!(current.true_coverage > 0.0 && current.depth().is_finite());
        }
    }

    #[test]
    fn ribbon_crossing_near_plane_clips_without_screen_explosion() {
        let mut renderer = Renderer::new(50, 25, false);
        let camera = Camera {
            position: Vec3::ZERO,
            target: Vec3::Z,
        };
        let ribbon = RoadRibbon {
            start: Vec3::new(0.0, 0.04, -1.0),
            end: Vec3::new(0.0, 0.04, 5.0),
            width: 0.18,
            color: PAINT,
            shade: 1.0,
        };
        renderer.draw_ribbons(&[ribbon], camera);
        assert!(
            renderer
                .markings
                .iter()
                .any(|mark| mark.true_coverage > 0.0)
        );
        assert!(
            renderer
                .markings
                .iter()
                .all(|mark| mark.true_coverage.is_finite()
                    && (mark.true_coverage == 0.0
                        || (mark.depth() >= NEAR && mark.depth() <= FAR)))
        );
        renderer.clear();
        let behind = RoadRibbon {
            end: Vec3::new(0.0, 0.04, -0.01),
            ..ribbon
        };
        renderer.draw_ribbons(&[behind], camera);
        assert!(
            renderer
                .markings
                .iter()
                .all(|mark| mark.true_coverage == 0.0)
        );
    }

    #[test]
    fn near_wide_paint_has_no_visibility_inflation() {
        let mut renderer = Renderer::new(50, 25, true);
        paint_strip(&mut renderer, Vec2::new(25.5, 12.5), 90.0, 0.7, 5.0);
        let marking = renderer.markings[CELL_INDEX];
        let coverage = marking.true_coverage;
        assert!(coverage > 0.5 && coverage < 0.9);
        assert!((ribbon_visual_width(0.7, 5.0) - 0.7).abs() < 1e-6);
        assert!(
            (marking_visibility(
                coverage,
                marking.assisted_coverage,
                marking.projected_width,
                5.0
            ) - coverage)
                .abs()
                < 1e-6
        );
        renderer.resolve();
        assert!(renderer.cells[CELL_INDEX].ch != ' ');
        assert!((renderer.cells[CELL_INDEX].depth - 5.0).abs() < 0.01);
        let paint = renderer.road_marking_cell(25, 12).unwrap();
        assert!((paint.projected_width - 0.7).abs() < 1e-5);
        assert!((paint.effective_coverage - paint.true_coverage).abs() < 1e-6);
    }

    #[test]
    fn foreshortened_paint_fades_with_distance() {
        let mut renderer = Renderer::new(50, 25, true);
        let mut red_levels = Vec::new();
        let mut coverages = Vec::new();
        let mut effective_coverages = Vec::new();
        for depth in [5.0_f32, 15.0, 40.0, 90.0, 165.0] {
            renderer.clear();
            // A shallow strip loses width roughly quadratically with distance.
            let width = 0.8 * (5.0 / depth).powi(2);
            paint_strip(&mut renderer, Vec2::new(25.5, 12.5), 0.0, width, depth);
            coverages.push(renderer.markings[CELL_INDEX].true_coverage);
            renderer.resolve();
            effective_coverages.push(
                renderer
                    .road_marking_cell(25, 12)
                    .unwrap()
                    .effective_coverage,
            );
            assert_ne!(renderer.cells[CELL_INDEX].ch, ' ');
            red_levels.push(renderer.cells[CELL_INDEX].color.0);
        }
        assert!(
            coverages.windows(2).all(|pair| pair[0] > pair[1]),
            "{coverages:?}"
        );
        assert!(
            red_levels.windows(2).all(|pair| pair[0] >= pair[1]),
            "{red_levels:?}"
        );
        assert!(
            effective_coverages.windows(2).all(|pair| pair[0] > pair[1]),
            "{effective_coverages:?}"
        );
        assert!(
            effective_coverages[0] > 0.5
                && effective_coverages[3] > 0.001
                && effective_coverages[4] < 0.001
        );
        assert!(
            red_levels[0] > red_levels[2] && red_levels[4] < 20,
            "{red_levels:?}"
        );
    }
    #[test]
    fn road_backed_marking_fades_without_a_score_cutoff() {
        let mut renderer = Renderer::new(50, 25, true);
        let mut contrasts = Vec::new();
        for depth in [5.0_f32, 15.0, 40.0, 90.0, 120.0, 165.0] {
            renderer.clear();
            let corners = [
                Vec2::new(25.0, 12.0),
                Vec2::new(25.0, 13.0),
                Vec2::new(26.0, 13.0),
                Vec2::new(26.0, 12.0),
            ]
            .map(|p| screen_vertex(p, depth + 0.03));
            renderer.rasterize(
                [corners[0], corners[1], corners[2]],
                0.6,
                (87, 91, 96),
                SurfaceKind::Road,
            );
            renderer.rasterize(
                [corners[0], corners[2], corners[3]],
                0.6,
                (87, 91, 96),
                SurfaceKind::Road,
            );
            renderer.resolve();
            let base = renderer.cells[CELL_INDEX].color;
            paint_strip(
                &mut renderer,
                Vec2::new(25.5, 12.5),
                0.0,
                0.8 * (5.0 / depth).powi(2),
                depth,
            );
            renderer.resolve();
            let cell = renderer.cells[CELL_INDEX];
            assert_ne!(cell.ch, ' ', "missing road background at {depth}");
            let difference = |a: u8, b: u8| (a as i16 - b as i16).abs();
            contrasts.push(
                difference(cell.color.0, base.0)
                    + difference(cell.color.1, base.1)
                    + difference(cell.color.2, base.2),
            );
        }
        assert!(
            contrasts.windows(2).all(|pair| pair[0] >= pair[1]),
            "{contrasts:?}"
        );
        assert!(
            contrasts[0] > contrasts[2]
                && contrasts[2] > contrasts[3]
                && contrasts[3] > contrasts[4]
                && contrasts[4] > contrasts[5],
            "{contrasts:?}"
        );
    }
    #[test]
    fn resolved_ribbons_follow_direction_without_losing_faint_support() {
        let mut renderer = Renderer::new(50, 25, true);
        for (degrees, expected) in [(90.0, '|'), (0.0, '-'), (-45.0, '/'), (45.0, '\\')] {
            renderer.clear();
            paint_strip(&mut renderer, Vec2::new(25.5, 12.5), degrees, 0.08, 5.0);
            renderer.resolve();
            assert_eq!(renderer.cells[CELL_INDEX].ch, expected, "{degrees}°");
            assert!(
                renderer
                    .road_marking_cell(25, 12)
                    .unwrap()
                    .effective_coverage
                    > 0.0
            );
        }
        renderer.clear();
        paint_strip(&mut renderer, Vec2::new(25.5, 12.5), -45.0, 0.015, 50.0);
        renderer.resolve();
        let paint = renderer.road_marking_cell(25, 12).unwrap();
        assert!(paint.effective_coverage > paint.true_coverage);
        assert_eq!(renderer.cells[CELL_INDEX].ch, '/');
        renderer.clear();
        renderer.resolve();
        assert_eq!(renderer.cells[CELL_INDEX].ch, ' ');
    }

    #[test]
    fn occluded_ribbon_does_not_steer_glyph_or_history() {
        let mut renderer = Renderer::new(50, 25, true);
        paint_strip(&mut renderer, Vec2::new(25.5, 12.5), -45.0, 0.08, 5.0);
        renderer.resolve();
        assert_eq!(renderer.cells[CELL_INDEX].ch, '/');
        renderer.clear();
        paint_strip(&mut renderer, Vec2::new(25.5, 12.5), -45.0, 0.08, 5.0);
        for sample in &mut renderer.samples[CELL_INDEX * 4..CELL_INDEX * 4 + 4] {
            *sample = Sample {
                depth: 3.0,
                color: (255, 0, 0),
                brightness: 0.8,
                surface: SurfaceKind::Vehicle,
                coverage: 1.0,
            };
        }
        renderer.resolve();
        assert_eq!(
            renderer
                .road_marking_cell(25, 12)
                .unwrap()
                .effective_coverage,
            0.0
        );
        assert_ne!(renderer.cells[CELL_INDEX].ch, '/');
        assert_eq!(
            renderer.glyph_history[CELL_INDEX].surface,
            SurfaceKind::Vehicle
        );
        renderer.clear();
        paint_strip(&mut renderer, Vec2::new(25.5, 12.5), 90.0, 0.08, 5.0);
        renderer.resolve();
        assert_eq!(renderer.cells[CELL_INDEX].ch, '|');
    }

    #[test]
    fn lighting_uses_rgb_without_rewriting_stable_silhouette() {
        let mut renderer = Renderer::new(50, 25, true);
        let shape = [
            screen_vertex(Vec2::new(25.0, 12.0), 5.0),
            screen_vertex(Vec2::new(25.0, 13.0), 5.0),
            screen_vertex(Vec2::new(25.49, 12.0), 5.0),
        ];
        let mut seen = Vec::new();
        for brightness in [0.65, 0.7, 0.75] {
            renderer.clear();
            renderer.rasterize(shape, brightness, (235, 170, 120), SurfaceKind::Vehicle);
            renderer.resolve();
            seen.push(renderer.cells[CELL_INDEX]);
        }
        assert!(seen.iter().all(|cell| "=+*#%@".contains(cell.ch)));
        assert!(seen.iter().all(|cell| cell.ch == seen[0].ch));
        assert!(
            seen.windows(2)
                .all(|pair| pair[0].color.0 < pair[1].color.0)
        );
    }

    #[test]
    fn resize_and_camera_jump_drop_previous_glyph() {
        let mut renderer = Renderer::new(50, 25, true);
        renderer.glyph_history[CELL_INDEX] = GlyphHistory {
            ch: '/',
            depth: 5.0,
            color: (120, 100, 80),
            brightness: 0.5,
            coverage: 0.2,
            surface: SurfaceKind::RoadMarking,
        };
        renderer.resize(50, 25);
        assert_eq!(renderer.glyph_history[CELL_INDEX].ch, ' ');
        renderer.glyph_history[CELL_INDEX].ch = '/';
        let camera = Camera {
            position: Vec3::ZERO,
            target: Vec3::Z,
        };
        renderer.observe_camera(camera);
        renderer.clear();
        renderer.observe_camera(Camera {
            position: Vec3::new(8.0, 0.0, 0.0),
            ..camera
        });
        assert_eq!(renderer.glyph_history[CELL_INDEX].ch, ' ');
    }
}
