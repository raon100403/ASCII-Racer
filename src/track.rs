use crate::mesh::{Mesh, RoadRibbon, SurfaceKind};
use glam::{Mat4, Vec2, Vec3};

pub const ROAD_HALF_WIDTH: f32 = 6.2;
const CHECKPOINT_RADIUS: f32 = 7.0;
const CHECKPOINTS: &[usize] = &[2, 4, 6, 8, 10, 0];
const RAIL_SEGMENTS: &[usize] = &[1, 2, 5, 6, 7];

pub struct Track {
    points: Vec<Vec2>,
    pub mesh: Mesh,
    pub ribbons: Vec<RoadRibbon>,
    next_checkpoint: usize,
    checkpoint_armed: bool,
    pub laps: u32,
}

impl Track {
    pub fn new() -> Self {
        let points = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(0.0, 28.0),
            Vec2::new(3.0, 51.0),
            Vec2::new(18.0, 65.0),
            Vec2::new(46.0, 68.0),
            Vec2::new(75.0, 64.0),
            Vec2::new(94.0, 47.0),
            Vec2::new(92.0, 23.0),
            Vec2::new(70.0, 8.0),
            Vec2::new(44.0, 3.0),
            Vec2::new(20.0, -8.0),
            Vec2::new(5.0, -14.0),
        ];
        let mut ribbons = Vec::new();
        let mesh = build_mesh(&points, &mut ribbons);
        Self {
            points,
            mesh,
            ribbons,
            next_checkpoint: 0,
            checkpoint_armed: true,
            laps: 0,
        }
    }

    pub fn reset_progress(&mut self) {
        self.next_checkpoint = 0;
        self.checkpoint_armed = true;
    }

    pub fn checkpoint_display(&self) -> usize {
        self.next_checkpoint + 1
    }
    pub fn checkpoint_count(&self) -> usize {
        CHECKPOINTS.len()
    }

    pub fn update_checkpoint(&mut self, position: Vec3) {
        let point = self.points[CHECKPOINTS[self.next_checkpoint]];
        let distance = Vec2::new(position.x, position.z).distance(point);
        if distance > CHECKPOINT_RADIUS + 3.0 {
            self.checkpoint_armed = true;
        }
        if distance < CHECKPOINT_RADIUS && self.checkpoint_armed {
            self.checkpoint_armed = false;
            self.next_checkpoint += 1;
            if self.next_checkpoint == CHECKPOINTS.len() {
                self.next_checkpoint = 0;
                self.laps += 1;
            }
        }
    }

    pub fn road_distance(&self, position: Vec3) -> f32 {
        let p = Vec2::new(position.x, position.z);
        self.segments()
            .map(|(a, b, _)| distance_to_segment(p, a, b).0)
            .fold(f32::INFINITY, f32::min)
    }

    pub fn collide(&self, position: &mut Vec3, velocity: &mut Vec3) {
        let mut p = Vec2::new(position.x, position.z);
        for (a, b, i) in self.segments() {
            if !RAIL_SEGMENTS.contains(&i) {
                continue;
            }
            let dir = (b - a).normalize();
            let right = Vec2::new(dir.y, -dir.x);
            for side in [-1.0, 1.0] {
                let offset = right * side * (ROAD_HALF_WIDTH + 1.1);
                let rail_a = a + offset;
                let rail_b = b + offset;
                let (distance, nearest) = distance_to_segment(p, rail_a, rail_b);
                let min_distance = 0.95;
                if distance < min_distance {
                    let normal = (p - nearest).normalize_or_zero();
                    if normal.length_squared() > 0.0 {
                        p = nearest + normal * min_distance;
                        let v = Vec2::new(velocity.x, velocity.z);
                        let into = v.dot(normal);
                        if into < 0.0 {
                            let corrected = (v - normal * into * 1.25) * 0.58;
                            velocity.x = corrected.x;
                            velocity.z = corrected.y;
                        }
                    }
                }
            }
        }
        position.x = p.x;
        position.z = p.y;
    }

    fn segments(&self) -> impl Iterator<Item = (Vec2, Vec2, usize)> + '_ {
        (0..self.points.len())
            .map(|i| (self.points[i], self.points[(i + 1) % self.points.len()], i))
    }
}

fn distance_to_segment(p: Vec2, a: Vec2, b: Vec2) -> (f32, Vec2) {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
    let nearest = a + ab * t;
    (p.distance(nearest), nearest)
}

fn build_mesh(points: &[Vec2], ribbons: &mut Vec<RoadRibbon>) -> Mesh {
    let mut mesh = Mesh::new();
    let grass = (40, 102, 46);
    let a = Vec3::new(-210.0, -0.08, -210.0);
    let b = Vec3::new(-210.0, -0.08, 210.0);
    let c = Vec3::new(210.0, -0.08, 210.0);
    let d = Vec3::new(210.0, -0.08, -210.0);
    mesh.quad_kind(a, b, c, d, grass, 0.34, SurfaceKind::Ground);
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        let dir = (b - a).normalize();
        let right = Vec2::new(dir.y, -dir.x);
        strip(
            &mut mesh,
            a,
            b,
            right,
            -ROAD_HALF_WIDTH - 0.55,
            ROAD_HALF_WIDTH + 0.55,
            0.0,
            (125, 118, 89),
            0.52,
            SurfaceKind::Ground,
        );
        strip(
            &mut mesh,
            a,
            b,
            right,
            -ROAD_HALF_WIDTH,
            ROAD_HALF_WIDTH,
            0.025,
            (87, 91, 96),
            0.62,
            SurfaceKind::Road,
        );
        for side in [-1.0, 1.0] {
            let offset = right * side * (ROAD_HALF_WIDTH - 0.09);
            ribbons.push(RoadRibbon {
                start: Vec3::new(a.x + offset.x, 0.04, a.y + offset.y),
                end: Vec3::new(b.x + offset.x, 0.04, b.y + offset.y),
                width: 0.18,
                color: (245, 225, 145),
                shade: 1.0,
            });
        }
        let mut distance = 4.0;
        let length = a.distance(b);
        while distance + 2.0 < length {
            let start = a + dir * distance;
            ribbons.push(RoadRibbon {
                start: Vec3::new(start.x, 0.048, start.y),
                end: Vec3::new(start.x + dir.x * 2.1, 0.048, start.y + dir.y * 2.1),
                width: 0.16,
                color: (225, 216, 176),
                shade: 0.95,
            });
            distance += 8.0;
        }
        if RAIL_SEGMENTS.contains(&i) {
            for side in [-1.0, 1.0] {
                let middle = (a + b) * 0.5 + right * side * (ROAD_HALF_WIDTH + 1.1);
                let box_mesh = Mesh::box_mesh(Vec3::new(0.28, 0.65, length), (202, 208, 218))
                    .with_surface(SurfaceKind::Guardrail);
                let yaw = dir.x.atan2(dir.y);
                mesh.append_transformed(
                    &box_mesh,
                    Mat4::from_translation(Vec3::new(middle.x, 0.43, middle.y))
                        * Mat4::from_rotation_y(yaw),
                );
            }
        }
    }
    // Circular joins keep the road continuous where two strip segments turn.
    for &point in points {
        let center = Vec3::new(point.x, 0.024, point.y);
        for slice in 0..16 {
            let angle_a = slice as f32 * std::f32::consts::TAU / 16.0;
            let angle_b = (slice + 1) as f32 * std::f32::consts::TAU / 16.0;
            let rim = |angle: f32| {
                center
                    + Vec3::new(
                        angle.cos() * ROAD_HALF_WIDTH,
                        0.0,
                        angle.sin() * ROAD_HALF_WIDTH,
                    )
            };
            mesh.add_kind(
                [center, rim(angle_b), rim(angle_a)],
                (87, 91, 96),
                0.62,
                SurfaceKind::Road,
            );
        }
    }
    // Bridge straight edge ribbon endpoints with arcs at the same center radius.
    for i in 0..points.len() {
        let point = points[i];
        let incoming = (point - points[(i + points.len() - 1) % points.len()]).normalize();
        let outgoing = (points[(i + 1) % points.len()] - point).normalize();
        let turn = incoming.perp_dot(outgoing);
        if turn.abs() < 0.001 {
            continue;
        }
        for side in [-1.0, 1.0] {
            let start = Vec2::new(incoming.y, -incoming.x) * side;
            let end = Vec2::new(outgoing.y, -outgoing.x) * side;
            let angle = start.perp_dot(end).atan2(start.dot(end));
            let steps = ((angle.abs() * 8.0).ceil() as usize).max(1);
            for step in 0..steps {
                let dir = |t: f32| {
                    let a = angle * t;
                    Vec2::new(
                        start.x * a.cos() - start.y * a.sin(),
                        start.x * a.sin() + start.y * a.cos(),
                    )
                };
                let at = |direction: Vec2, radius: f32| {
                    Vec3::new(
                        point.x + direction.x * radius,
                        0.04,
                        point.y + direction.y * radius,
                    )
                };
                let u = dir(step as f32 / steps as f32);
                let v = dir((step + 1) as f32 / steps as f32);
                let start = at(u, ROAD_HALF_WIDTH - 0.09);
                let end = at(v, ROAD_HALF_WIDTH - 0.09);
                ribbons.push(RoadRibbon {
                    start,
                    end,
                    width: 0.18,
                    color: (245, 225, 145),
                    shade: 1.0,
                });
            }
        }
    }
    for &index in CHECKPOINTS {
        let p = points[index];
        let previous = points[(index + points.len() - 1) % points.len()];
        let next = points[(index + 1) % points.len()];
        let dir = (next - previous).normalize();
        let right = Vec2::new(dir.y, -dir.x);
        strip(
            &mut mesh,
            p - dir * 0.28,
            p + dir * 0.28,
            right,
            -ROAD_HALF_WIDTH,
            ROAD_HALF_WIDTH,
            0.065,
            (95, 230, 230),
            1.0,
            SurfaceKind::Checkpoint,
        );
        for side in [-1.0, 1.0] {
            let pos = p + right * side * (ROAD_HALF_WIDTH + 0.55);
            let pillar = Mesh::box_mesh(Vec3::new(0.48, 2.2, 0.48), (80, 225, 235))
                .with_surface(SurfaceKind::Checkpoint);
            mesh.append_transformed(
                &pillar,
                Mat4::from_translation(Vec3::new(pos.x, 1.1, pos.y)),
            );
        }
    }
    mesh
}

fn strip(
    mesh: &mut Mesh,
    a: Vec2,
    b: Vec2,
    right: Vec2,
    left: f32,
    right_edge: f32,
    y: f32,
    color: (u8, u8, u8),
    shade: f32,
    surface: SurfaceKind,
) {
    let p = |point: Vec2, offset: f32| {
        Vec3::new(point.x + right.x * offset, y, point.y + right.y * offset)
    };
    mesh.quad_kind(
        p(a, left),
        p(b, left),
        p(b, right_edge),
        p(a, right_edge),
        color,
        shade,
        surface,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checkpoints_require_order() {
        let mut track = Track::new();
        track.update_checkpoint(Vec3::new(46.0, 0.0, 68.0));
        assert_eq!(track.checkpoint_display(), 1);
        for &i in CHECKPOINTS {
            let p = track.points[i];
            track.update_checkpoint(Vec3::new(200.0, 0.0, 200.0));
            track.update_checkpoint(Vec3::new(p.x, 0.0, p.y));
        }
        assert_eq!(track.laps, 1);
    }

    #[test]
    fn corner_ribbons_connect_straight_edges() {
        let track = Track::new();
        assert!(
            track
                .mesh
                .triangles
                .iter()
                .all(|triangle| triangle.surface != SurfaceKind::RoadMarking)
        );
        let center_radius = ROAD_HALF_WIDTH - 0.09;
        for i in 0..track.points.len() {
            let point = track.points[i];
            let incoming = (point
                - track.points[(i + track.points.len() - 1) % track.points.len()])
            .normalize();
            let outgoing = (track.points[(i + 1) % track.points.len()] - point).normalize();
            for side in [-1.0, 1.0] {
                let tip = |direction: Vec2| {
                    point + Vec2::new(direction.y, -direction.x) * side * center_radius
                };
                let from = tip(incoming);
                let to = tip(outgoing);
                let start = Vec3::new(from.x, 0.04, from.y);
                let end = Vec3::new(to.x, 0.04, to.y);
                let angle = incoming.perp_dot(outgoing).atan2(incoming.dot(outgoing));
                let steps = ((angle.abs() * 8.0).ceil() as usize).max(1);
                let first = track.ribbons.iter().position(|r| {
                    r.start.distance(start) < 0.001 && (r.start.y - 0.04).abs() < 0.0001
                });
                let first =
                    first.unwrap_or_else(|| panic!("missing corner ribbon at {i}, side {side}"));
                let arc = track
                    .ribbons
                    .get(first..first + steps)
                    .expect("incomplete corner ribbon");
                assert!(
                    arc.last().unwrap().end.distance(end) < 0.001,
                    "outgoing edge does not meet corner {i}, side {side}"
                );
                for r in arc {
                    assert!((r.start.y - 0.04).abs() < 0.0001);
                    assert!((r.width - 0.18).abs() < 0.0001);
                    assert!(
                        ((Vec2::new(r.start.x, r.start.z) - point).length() - center_radius).abs()
                            < 0.001
                    );
                }
                for pair in arc.windows(2) {
                    assert!(
                        pair[0].end.distance(pair[1].start) < 0.001,
                        "disconnected corner at {i}, side {side}"
                    );
                }
                assert!(
                    track.ribbons.iter().any(|r| {
                        r.end.distance(start) < 0.001 && r.start.distance(start) > 0.3
                    })
                );
                assert!(
                    track
                        .ribbons
                        .iter()
                        .any(|r| { r.start.distance(end) < 0.001 && r.end.distance(end) > 0.3 })
                );
            }
        }
    }

    #[test]
    fn center_dashes_follow_two_point_one_length_and_eight_unit_step() {
        let track = Track::new();
        for (a, b, _) in track.segments() {
            let dir = (b - a).normalize();
            let length = a.distance(b);
            let dashes: Vec<_> = track
                .ribbons
                .iter()
                .filter(|r| {
                    if (r.start.y - 0.048).abs() > 0.0001 {
                        return false;
                    }
                    let relative = Vec2::new(r.start.x, r.start.z) - a;
                    relative.perp_dot(dir).abs() < 0.001
                        && relative.dot(dir) >= 3.999
                        && relative.dot(dir) < length - 2.0 + 0.001
                })
                .collect();
            let mut expected = 4.0;
            for dash in &dashes {
                let start = Vec2::new(dash.start.x, dash.start.z);
                let end = Vec2::new(dash.end.x, dash.end.z);
                assert!(start.distance(a + dir * expected) < 0.001);
                assert!(end.distance(start + dir * 2.1) < 0.001);
                assert!((dash.width - 0.16).abs() < 0.0001);
                expected += 8.0;
            }
            assert!(expected + 2.0 >= length);
        }
    }
}
