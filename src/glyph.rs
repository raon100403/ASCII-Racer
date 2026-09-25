//! Small font-independent glyph profiles for the final character-cell resolve.
use crate::mesh::SurfaceKind;
use glam::Vec2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlyphMode {
    Shape,
    Density,
}

#[derive(Clone, Copy)]
struct GlyphInfo {
    ch: char,
    density: f32,
    // Coarse top-left, top-right, bottom-left, bottom-right ink profile.
    mask: [f32; 4],
    direction: Vec2, // zero for non-directional density glyphs
    structural: bool,
}

const fn glyph(
    ch: char,
    density: f32,
    mask: [f32; 4],
    direction: Vec2,
    structural: bool,
) -> GlyphInfo {
    GlyphInfo {
        ch,
        density,
        mask,
        direction,
        structural,
    }
}

const H: Vec2 = Vec2::new(1.0, 0.0);
const V: Vec2 = Vec2::new(0.0, 1.0);
const DOWN: Vec2 = Vec2::new(0.70710677, 0.70710677);
const UP: Vec2 = Vec2::new(0.70710677, -0.70710677);
// Profiles intentionally describe character families, not a particular terminal font.
const GLYPHS: [GlyphInfo; 16] = [
    glyph(' ', 0.0, [0.0; 4], Vec2::ZERO, false),
    glyph('.', 0.09, [0.0, 0.0, 0.25, 0.25], Vec2::ZERO, false),
    glyph(',', 0.14, [0.0, 0.0, 0.4, 0.2], Vec2::ZERO, false),
    glyph(':', 0.22, [0.25; 4], Vec2::ZERO, false),
    glyph(';', 0.28, [0.2, 0.2, 0.5, 0.2], Vec2::ZERO, false),
    glyph('=', 0.38, [0.5; 4], H, false),
    glyph('+', 0.46, [0.5; 4], Vec2::ZERO, false),
    glyph('*', 0.57, [0.65; 4], Vec2::ZERO, false),
    glyph('#', 0.72, [0.8; 4], Vec2::ZERO, false),
    glyph('%', 0.84, [0.95, 0.55, 0.55, 0.95], Vec2::ZERO, false),
    glyph('@', 0.97, [1.0; 4], Vec2::ZERO, false),
    glyph('-', 0.24, [0.55; 4], H, true),
    glyph('|', 0.26, [0.55; 4], V, true),
    glyph('_', 0.23, [0.0, 0.0, 0.9, 0.9], H, true),
    glyph('/', 0.28, [0.0, 0.85, 0.85, 0.0], UP, true),
    glyph('\\', 0.28, [0.85, 0.0, 0.0, 0.85], DOWN, true),
];

// Mesh geometry enters one of two glyph families after raster topology resolve.
const CALM_SPARSE: &[u8] = &[1, 2, 3];
const CALM_OPAQUE: &[u8] = &[3];
const MARKING: &[u8] = &[1, 2, 11, 12, 13, 14, 15, 5, 6, 7, 8];
const FILLED: &[u8] = &[5, 6, 7, 8, 9, 10];
const THIN: &[u8] = &[5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
const CONTINUOUS_HORIZONTAL: &[u8] = &[5];

struct GlyphTuning {
    shape_weight: f32,
    density_weight: f32,
    brightness_weight: f32,
    direction_weight: f32,
    hysteresis_margin: f32,
    broad_width: f32,
    edge_threshold: f32,
    calm_coverage: f32,
    filled_density_floor: f32,
    structural_penalty: f32,
    horizontal_penalty: f32,
    thin_line_bonus: f32,
}
const TUNING: GlyphTuning = GlyphTuning {
    shape_weight: 0.62,
    density_weight: 0.7,
    brightness_weight: 0.09,
    direction_weight: 1.0,
    hysteresis_margin: 0.075,
    broad_width: 0.9,
    edge_threshold: 0.18,
    calm_coverage: 0.18,
    filled_density_floor: 0.43,
    structural_penalty: 0.65,
    horizontal_penalty: 0.42,
    thin_line_bonus: 0.22,
};

#[derive(Clone, Copy)]
pub(crate) struct GlyphInput {
    pub surface: SurfaceKind,
    pub coverage: f32,
    pub true_coverage: f32,
    pub projected_width: f32,
    pub intensity: f32,
    pub shape: [f32; 4],
    pub direction: Vec2,
    pub center_y: f32,
    pub role: CellRole,
    pub continuous_horizontal: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CellRole {
    Filled,
    SilhouetteBoundary,
    ThinStructure,
}

fn candidates(input: GlyphInput, role: CellRole) -> &'static [u8] {
    if input.continuous_horizontal
        && role == CellRole::ThinStructure
        && input.surface != SurfaceKind::RoadMarking
    {
        return CONTINUOUS_HORIZONTAL;
    }
    match input.surface {
        SurfaceKind::Road | SurfaceKind::Ground => {
            if input.coverage < TUNING.calm_coverage {
                CALM_SPARSE
            } else {
                CALM_OPAQUE
            }
        }
        SurfaceKind::RoadMarking => MARKING,
        _ if role == CellRole::ThinStructure => THIN,
        _ => FILLED,
    }
}

#[derive(Clone, Copy)]
struct Features {
    edge: f32,
    target: f32,
    directed: bool,
    marking: bool,
    calm: bool,
    line_penalty: f32,
    fill_penalty: f32,
    structural_eligibility: f32,
}

fn score(g: GlyphInfo, input: GlyphInput, f: Features) -> f32 {
    let density = TUNING.density_weight * (g.density - f.target).abs();
    let brightness = TUNING.brightness_weight * (g.density - input.intensity).abs();
    if f.calm {
        return density + brightness + if g.structural { 0.18 } else { 0.0 };
    }
    let shape = if f.marking || f.edge < TUNING.edge_threshold {
        0.0
    } else {
        let mismatch = (0..4)
            .map(|i| (g.mask[i] - input.shape[i]).abs())
            .sum::<f32>()
            * 0.25;
        TUNING.shape_weight * f.edge * mismatch
    };
    let direction = if f.directed && g.structural {
        // Undirected lines: |dot|² avoids angle buckets and trigonometry.
        let similarity = input.direction.dot(g.direction).powi(2).clamp(0.0, 1.0);
        TUNING.direction_weight * (1.0 - similarity) * if f.marking { 0.9 } else { 0.24 * f.edge }
    } else {
        0.0
    };
    let family = if f.marking {
        if g.structural {
            f.line_penalty
        } else if f.directed {
            f.fill_penalty
        } else {
            0.0
        }
    } else if g.structural {
        TUNING.structural_penalty * (1.0 - f.structural_eligibility)
            + if matches!(g.ch, '-' | '_') {
                TUNING.horizontal_penalty * (input.coverage - 0.55).max(0.0)
            } else {
                0.0
            }
    } else {
        0.0
    };
    let baseline = if f.marking && g.ch == '_' {
        (0.65 - input.center_y).max(0.0) * 0.15
    } else {
        0.0
    };
    let thin_line_bonus = if f.structural_eligibility > 0.7
        && ((g.ch == '-'
            && input.shape[0] > 0.5
            && input.shape[1] > 0.5
            && input.shape[2] < 0.5
            && input.shape[3] < 0.5)
            || (g.ch == '|'
                && ((input.shape[0] > 0.5
                    && input.shape[2] > 0.5
                    && input.shape[1] < 0.5
                    && input.shape[3] < 0.5)
                    || (input.shape[1] > 0.5
                        && input.shape[3] > 0.5
                        && input.shape[0] < 0.5
                        && input.shape[2] < 0.5))))
    {
        TUNING.thin_line_bonus
    } else {
        0.0
    };
    density + brightness + shape + direction + family + baseline - thin_line_bonus
}

pub(crate) fn choose(input: GlyphInput, previous: Option<char>) -> char {
    if input.coverage <= 0.0
        || (input.surface == SurfaceKind::RoadMarking && input.true_coverage <= 0.0)
    {
        return ' ';
    }
    let marking = input.surface == SurfaceKind::RoadMarking;
    let calm = matches!(input.surface, SurfaceKind::Road | SurfaceKind::Ground);
    let broad = if marking {
        (input.coverage * 0.65 + input.projected_width * 0.35).clamp(0.0, 1.0)
    } else {
        input.coverage.clamp(0.0, 1.0)
    };
    let edge = if marking || calm {
        0.0
    } else {
        let [tl, tr, bl, br] = input.shape;
        tl.max(tr).max(bl).max(br) - tl.min(tr).min(bl).min(br)
    };
    let role = if marking && input.projected_width < TUNING.broad_width {
        CellRole::ThinStructure
    } else {
        input.role
    };
    let f = Features {
        edge,
        target: if input.surface == SurfaceKind::Road {
            0.1 + 0.24 * input.intensity
        } else if input.surface == SurfaceKind::Ground {
            0.12 + 0.36 * input.intensity
        } else if marking {
            0.16 + 0.65 * broad
        } else {
            (0.12 + 0.7 * broad).max(if role == CellRole::ThinStructure {
                0.0
            } else {
                TUNING.filled_density_floor
            })
        },
        directed: marking && input.direction.length_squared() > 0.05,
        marking,
        calm,
        line_penalty: (broad - 0.62).max(0.0) * 2.0,
        fill_penalty: (TUNING.broad_width - broad).max(0.0) * 0.63,
        structural_eligibility: if role == CellRole::ThinStructure {
            1.0 - (input.coverage - 0.4).max(0.0) * 0.5
        } else {
            0.0
        },
    };
    let mut best = (' ', f32::INFINITY);
    let mut old = f32::INFINITY;
    for &index in candidates(input, role) {
        let g = GLYPHS[index as usize];
        let error = score(g, input, f);
        if error < best.1 {
            best = (g.ch, error);
        }
        if previous == Some(g.ch) {
            old = error;
        }
    }
    if old <= best.1 + TUNING.hysteresis_margin {
        previous.unwrap()
    } else {
        best.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn marking(direction: Vec2, coverage: f32) -> GlyphInput {
        GlyphInput {
            surface: SurfaceKind::RoadMarking,
            coverage,
            true_coverage: coverage * 0.5,
            projected_width: coverage,
            intensity: 0.35,
            shape: [0.0; 4],
            direction,
            center_y: 0.5,
            role: CellRole::Filled,
            continuous_horizontal: false,
        }
    }
    #[test]
    fn ribbons_follow_projected_direction() {
        assert_eq!(choose(marking(V, 0.24), None), '|');
        assert!(matches!(choose(marking(H, 0.24), None), '-' | '_'));
        assert_eq!(choose(marking(UP, 0.24), None), '/');
        assert_eq!(choose(marking(DOWN, 0.24), None), '\\');
    }
    #[test]
    fn broad_paint_and_faint_true_support() {
        let mut faint = marking(UP, 0.04);
        faint.true_coverage = 0.004;
        assert_eq!(choose(faint, None), '/');
        faint.true_coverage = 0.0;
        assert_eq!(choose(faint, Some('/')), ' ');
        let wide = marking(UP, 1.0);
        assert!(matches!(choose(wide, None), '#' | '*' | '='));
    }
    #[test]
    fn flat_road_avoids_structural_noise() {
        let road = GlyphInput {
            surface: SurfaceKind::Road,
            coverage: 1.0,
            true_coverage: 1.0,
            projected_width: 0.0,
            intensity: 0.3,
            shape: [1.0; 4],
            direction: H,
            center_y: 0.5,
            role: CellRole::Filled,
            continuous_horizontal: false,
        };
        assert_eq!(choose(road, None), ':');
    }
    #[test]
    fn near_ties_hold_previous_but_new_geometry_changes_it() {
        let near = marking(Vec2::new(0.34, -0.94).normalize(), 0.24);
        assert_eq!(choose(near, None), '|');
        assert_eq!(choose(near, Some('/')), '/');
        assert_eq!(choose(marking(H, 0.24), Some('/')), '-');
    }
    fn solid(surface: SurfaceKind, shape: [f32; 4], role: CellRole) -> GlyphInput {
        GlyphInput {
            surface,
            coverage: shape.iter().sum::<f32>() * 0.25,
            true_coverage: 1.0,
            projected_width: 0.0,
            intensity: 0.28,
            shape,
            direction: Vec2::ZERO,
            center_y: 0.5,
            role,
            continuous_horizontal: false,
        }
    }

    #[test]
    fn filled_vehicle_corners_are_not_ascii_container_edges() {
        for shape in [
            [0.0, 1.0, 1.0, 1.0],
            [1.0, 0.0, 1.0, 1.0],
            [0.0, 1.0, 1.0, 0.0],
            [1.0, 0.0, 0.0, 1.0],
        ] {
            let input = solid(SurfaceKind::Vehicle, shape, CellRole::SilhouetteBoundary);
            assert!("=+*#%@".contains(choose(input, None)), "{shape:?}");
            assert!(
                "=+*#%@".contains(choose(input, Some('/'))),
                "stuck slash {shape:?}"
            );
        }
    }

    #[test]
    fn vehicle_sides_and_broad_horizontal_faces_remain_filled() {
        for shape in [
            [1.0; 4],
            [0.0, 0.0, 1.0, 1.0],
            [1.0, 0.0, 1.0, 0.0],
            [0.0, 1.0, 0.0, 1.0],
        ] {
            let input = solid(SurfaceKind::Vehicle, shape, CellRole::SilhouetteBoundary);
            assert!("=+*#%@".contains(choose(input, Some('-'))), "{shape:?}");
        }
    }

    #[test]
    fn generic_roof_stripe_is_not_a_horizontal_wire() {
        let top = solid(
            SurfaceKind::Generic,
            [1.0, 1.0, 0.0, 0.0],
            CellRole::SilhouetteBoundary,
        );
        assert!("=+*#%@".contains(choose(top, None)));
        let backed = solid(
            SurfaceKind::Generic,
            [0.0, 1.0, 1.0, 0.0],
            CellRole::SilhouetteBoundary,
        );
        assert!("=+*#%@".contains(choose(backed, Some('/'))));
    }

    #[test]
    fn broad_guardrail_boundary_does_not_become_a_dashed_rail() {
        let backed = solid(
            SurfaceKind::Guardrail,
            [1.0, 1.0, 0.0, 0.0],
            CellRole::SilhouetteBoundary,
        );
        assert!("=+*#%@".contains(choose(backed, Some('-'))));
        let broad = solid(SurfaceKind::Guardrail, [1.0; 4], CellRole::Filled);
        assert!("=+*#%@".contains(choose(broad, None)));
    }

    #[test]
    fn standalone_thin_diagonal_and_horizontal_features_still_have_direction() {
        assert_eq!(
            choose(
                solid(
                    SurfaceKind::Generic,
                    [0.0, 1.0, 1.0, 0.0],
                    CellRole::ThinStructure
                ),
                None
            ),
            '/'
        );
        assert_eq!(
            choose(
                solid(
                    SurfaceKind::Generic,
                    [1.0, 0.0, 0.0, 1.0],
                    CellRole::ThinStructure
                ),
                None
            ),
            '\\'
        );
        assert!(matches!(
            choose(
                solid(
                    SurfaceKind::Guardrail,
                    [1.0, 1.0, 0.0, 0.0],
                    CellRole::ThinStructure
                ),
                None
            ),
            '-' | '_'
        ));
    }

    #[test]
    fn dim_but_opaque_surfaces_keep_fill_ink() {
        for surface in [SurfaceKind::Vehicle, SurfaceKind::Generic] {
            let mut input = solid(surface, [1.0; 4], CellRole::Filled);
            input.intensity = 0.02; // RGB may be dim; coverage is still opaque.
            assert!("=+*#%@".contains(choose(input, Some(','))));
        }
        let mut road = solid(SurfaceKind::Road, [1.0; 4], CellRole::Filled);
        road.intensity = 0.02;
        assert_eq!(choose(road, Some(',')), ':');
        road.coverage = 0.0;
        assert_eq!(choose(road, Some(':')), ' ');
    }
}
