use crate::mesh::Mesh;
use glam::{Mat4, Vec2, Vec3};

pub const ROAD_HALF_WIDTH: f32 = 6.2;
const CHECKPOINT_RADIUS: f32 = 7.0;
const CHECKPOINTS: &[usize] = &[2, 4, 6, 8, 10, 0];
const RAIL_SEGMENTS: &[usize] = &[1, 2, 5, 6, 7];

pub struct Track {
    points: Vec<Vec2>,
    pub mesh: Mesh,
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
        let mesh = build_mesh(&points);
        Self {
            points,
            mesh,
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

fn build_mesh(points: &[Vec2]) -> Mesh {
    let mut mesh = Mesh::new();
    let grass = (40, 102, 46);
    let a = Vec3::new(-210.0, -0.08, -210.0);
    let b = Vec3::new(-210.0, -0.08, 210.0);
    let c = Vec3::new(210.0, -0.08, 210.0);
    let d = Vec3::new(210.0, -0.08, -210.0);
    mesh.quad(a, b, c, d, grass, 0.34);
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
        );
        strip(
            &mut mesh,
            a,
            b,
            right,
            -ROAD_HALF_WIDTH,
            -ROAD_HALF_WIDTH + 0.18,
            0.04,
            (245, 225, 145),
            1.0,
        );
        strip(
            &mut mesh,
            a,
            b,
            right,
            ROAD_HALF_WIDTH - 0.18,
            ROAD_HALF_WIDTH,
            0.04,
            (245, 225, 145),
            1.0,
        );
        let mut distance = 4.0;
        let length = a.distance(b);
        while distance + 2.0 < length {
            let start = a + dir * distance;
            strip(
                &mut mesh,
                start,
                start + dir * 2.1,
                right,
                -0.08,
                0.08,
                0.048,
                (225, 216, 176),
                0.95,
            );
            distance += 8.0;
        }
        if RAIL_SEGMENTS.contains(&i) {
            for side in [-1.0, 1.0] {
                let middle = (a + b) * 0.5 + right * side * (ROAD_HALF_WIDTH + 1.1);
                let box_mesh = Mesh::box_mesh(Vec3::new(0.28, 0.65, length), (202, 208, 218));
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
            mesh.add([center, rim(angle_b), rim(angle_a)], (87, 91, 96), 0.62);
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
        );
        for side in [-1.0, 1.0] {
            let pos = p + right * side * (ROAD_HALF_WIDTH + 0.55);
            let pillar = Mesh::box_mesh(Vec3::new(0.48, 2.2, 0.48), (80, 225, 235));
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
) {
    let p = |point: Vec2, offset: f32| {
        Vec3::new(point.x + right.x * offset, y, point.y + right.y * offset)
    };
    mesh.quad(
        p(a, left),
        p(b, left),
        p(b, right_edge),
        p(a, right_edge),
        color,
        shade,
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
}
