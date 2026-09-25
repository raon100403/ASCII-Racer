use crate::{
    car::{Car, MAX_FORWARD_SPEED},
    renderer::Camera,
};
use glam::Vec3;

struct ChaseCameraTuning {
    base_distance: f32,
    max_speed_pullback: f32,
    height: f32,
    target_height: f32,
    base_look_ahead: f32,
    max_extra_look_ahead: f32,
    position_follow_rate: f32,
    heading_follow_rate: f32,
}

const CHASE: ChaseCameraTuning = ChaseCameraTuning {
    base_distance: 4.9,
    max_speed_pullback: 0.5,
    height: 3.0,
    target_height: 0.85,
    base_look_ahead: 3.0,
    max_extra_look_ahead: 1.0,
    position_follow_rate: 18.0,
    heading_follow_rate: 8.0,
};

pub struct ChaseCamera {
    camera: Camera,
    heading: Vec3,
}

impl ChaseCamera {
    pub fn new(car: &Car) -> Self {
        let heading = car.forward();
        Self {
            camera: Camera {
                position: car.position - heading * CHASE.base_distance + Vec3::Y * CHASE.height,
                target: car.position
                    + heading * CHASE.base_look_ahead
                    + Vec3::Y * CHASE.target_height,
            },
            heading,
        }
    }

    pub fn camera(&self) -> Camera {
        self.camera
    }

    pub fn update(&mut self, car: &Car, dt: f32) {
        let heading_alpha = 1.0 - (-CHASE.heading_follow_rate * dt).exp();
        self.heading = self.heading.lerp(car.forward(), heading_alpha).normalize();

        // Forward travel drives the framing; sideways drift and reverse do not.
        let speed_ratio =
            (car.velocity.dot(car.forward()).max(0.0) / MAX_FORWARD_SPEED).clamp(0.0, 1.0);
        let speed_curve = speed_ratio * speed_ratio * (3.0 - 2.0 * speed_ratio);
        let distance = CHASE.base_distance + CHASE.max_speed_pullback * speed_curve;
        let look_ahead = CHASE.base_look_ahead + CHASE.max_extra_look_ahead * speed_curve;
        let desired = car.position - self.heading * distance + Vec3::Y * CHASE.height;
        let position_alpha = 1.0 - (-CHASE.position_follow_rate * dt).exp();
        self.camera.position = self.camera.position.lerp(desired, position_alpha);
        // Track the car's current position so acceleration cannot eat the
        // intended road look-ahead; only heading changes are smoothed.
        self.camera.target =
            car.position + self.heading * look_ahead + Vec3::Y * CHASE.target_height;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::Renderer;
    use crate::track::{ROAD_HALF_WIDTH, Track};
    use glam::Vec3Swizzles;

    fn car_cells(car: &Car, camera: Camera) -> (usize, usize, usize) {
        let mut renderer = Renderer::new(100, 34, false);
        renderer.draw_mesh(&car.mesh(), camera);
        let mut packed = Vec::new();
        renderer.write_packed_cells(&mut packed);
        let visible: Vec<_> = packed
            .iter()
            .enumerate()
            .filter(|(_, pixel)| (**pixel & 0xff) != b' ' as u32)
            .map(|(index, _)| index / 100)
            .collect();
        (
            visible.len(),
            *visible.iter().min().unwrap_or(&34),
            *visible.iter().max().unwrap_or(&0),
        )
    }

    #[test]
    fn standing_camera_makes_the_car_larger_without_clipping_it() {
        let car = Car::new();
        let camera = ChaseCamera::new(&car).camera();
        let old = Camera {
            position: car.position - car.forward() * 6.3 + Vec3::Y * 3.7,
            target: car.position + car.forward() * 3.0 + Vec3::Y * 0.8,
        };
        let (new_count, top, bottom) = car_cells(&car, camera);
        let (old_count, _, _) = car_cells(&car, old);
        eprintln!("standing: old {old_count} car cells, new {new_count}, rows {top}..{bottom}");
        assert!(new_count > old_count * 6 / 5);
        assert!(top > 0 && bottom < 33);
    }

    #[test]
    fn acceleration_top_speed_and_braking_keep_a_close_stable_view() {
        let mut car = Car::new();
        let mut chase = ChaseCamera::new(&car);
        let dt = 1.0 / 60.0;
        let mut largest_gap = 0.0_f32;
        let mut top_speed_gap = 0.0;
        let mut top_speed_cells = 0;
        let mut old_top_speed_cells = 0;
        let mut max_step = 0.0_f32;
        let mut previous_gap = CHASE.base_distance;
        let mut old_camera = Camera {
            position: car.position - car.forward() * 6.3 + Vec3::Y * 3.7,
            target: car.position + car.forward() * 3.0 + Vec3::Y * 0.8,
        };
        for frame in 0..780 {
            let throttle = if frame < 600 { 1.0 } else { -1.0 };
            car.update(dt, throttle, 0.0, false, false);
            chase.update(&car, dt);
            let old_alpha = 1.0 - (-5.0 * dt).exp();
            old_camera.position = old_camera.position.lerp(
                car.position - car.forward() * 6.3 + Vec3::Y * 3.7,
                old_alpha,
            );
            old_camera.target = old_camera.target.lerp(
                car.position + car.forward() * 3.0 + Vec3::Y * 0.8,
                old_alpha,
            );
            let camera = chase.camera();
            let gap = (camera.position - car.position).xz().length();
            largest_gap = largest_gap.max(gap);
            max_step = max_step.max((gap - previous_gap).abs());
            previous_gap = gap;
            if frame == 599 {
                top_speed_gap = gap;
                top_speed_cells = car_cells(&car, camera).0;
                old_top_speed_cells = car_cells(&car, old_camera).0;
                let look_ahead = (camera.target - car.position).dot(car.forward());
                assert!((3.9..4.01).contains(&look_ahead));
            }
        }
        eprintln!(
            "straight: top {:.2} m/s, gap {:.2} m, old {} / new {} car cells; max gap {:.2} m, max frame change {:.3} m",
            MAX_FORWARD_SPEED,
            top_speed_gap,
            old_top_speed_cells,
            top_speed_cells,
            largest_gap,
            max_step
        );
        assert!(top_speed_gap < 7.0);
        assert!(largest_gap < 7.1);
        assert!(top_speed_cells >= 25);
        assert!(top_speed_cells > old_top_speed_cells * 3 / 2);
        assert!(max_step < 0.12);
    }

    #[test]
    fn two_fast_corners_keep_the_car_in_frame_and_distance_bounded() {
        let mut car = Car::new();
        let mut chase = ChaseCamera::new(&car);
        let dt = 1.0 / 60.0;
        let mut max_gap = 0.0_f32;
        let mut min_cells = usize::MAX;
        for frame in 0..180 {
            // Keep a full-speed approach while alternating hard left/right
            // inputs; this stresses camera orbit and positional follow.
            car.velocity = car.forward() * MAX_FORWARD_SPEED;
            let steer = if frame < 60 { 1.0 } else { -1.0 };
            car.update(dt, 1.0, steer, false, false);
            chase.update(&car, dt);
            let camera = chase.camera();
            let gap = (camera.position - car.position).xz().length();
            max_gap = max_gap.max(gap);
            if frame % 30 == 29 {
                let (count, top, bottom) = car_cells(&car, camera);
                min_cells = min_cells.min(count);
                assert!(top > 0 && bottom < 34, "frame {frame}: {top}..{bottom}");
                assert!((camera.target - car.position).dot(car.forward()) > 3.0);
            }
        }
        eprintln!("two fast corners: max gap {max_gap:.2} m, minimum {min_cells} car cells");
        assert!(max_gap < 8.0);
        assert!(min_cells >= 20);
    }

    #[test]
    fn track_straight_and_two_corners_show_road_markings_ahead() {
        let track = Track::new();
        for (label, position, yaw, speed) in [
            ("low speed straight", Vec3::new(0.0, 0.0, 20.0), 0.0, 7.0),
            (
                "maximum speed straight",
                Vec3::new(0.0, 0.0, 20.0),
                0.0,
                27.0,
            ),
            (
                "first corner",
                Vec3::new(3.0, 0.0, 51.0),
                15.0_f32.atan2(14.0),
                18.0,
            ),
            (
                "second corner",
                Vec3::new(46.0, 0.0, 68.0),
                29.0_f32.atan2(-4.0),
                18.0,
            ),
        ] {
            let mut car = Car::new();
            car.yaw = yaw;
            car.velocity = car.forward() * speed;
            car.position = position - car.velocity;
            let mut chase = ChaseCamera::new(&car);
            for _ in 0..60 {
                car.position += car.velocity / 60.0;
                chase.update(&car, 1.0 / 60.0);
            }
            let mut renderer = Renderer::new(100, 34, false);
            let camera = chase.camera();
            renderer.draw_mesh(&track.mesh, camera);
            renderer.draw_ribbons(&track.ribbons, camera);
            renderer.draw_mesh(&car.mesh(), camera);
            let mut packed = Vec::new();
            renderer.write_packed_cells(&mut packed);
            let markings_ahead = (0..22)
                .flat_map(|y| (0..100).map(move |x| (x, y)))
                .filter(|&(x, y)| {
                    renderer
                        .road_marking_cell(x, y)
                        .is_some_and(|cell| cell.effective_coverage > 0.0)
                })
                .count();
            if std::env::var_os("ASCII_RACER_CAMERA_PREVIEW").is_some() {
                eprintln!("{label} — {markings_ahead} marking cells ahead");
                for row in packed.chunks(100) {
                    let line: String = row
                        .iter()
                        .map(|value| (value & 0xff) as u8 as char)
                        .collect();
                    eprintln!("{line}");
                }
            }
            assert!(
                markings_ahead > 5,
                "{label}: only {markings_ahead} marking cells"
            );
        }
    }

    #[test]
    fn scripted_drive_reaches_speed_and_follows_multiple_track_corners() {
        let track = Track::new();
        let waypoints = [
            (0.0_f32, 28.0_f32),
            (3.0, 51.0),
            (18.0, 65.0),
            (46.0, 68.0),
            (75.0, 64.0),
            (94.0, 47.0),
            (92.0, 23.0),
            (70.0, 8.0),
            (44.0, 3.0),
            (20.0, -8.0),
            (5.0, -14.0),
        ];
        let mut car = Car::new();
        let mut chase = ChaseCamera::new(&car);
        let mut next = 0;
        let mut max_speed = 0.0_f32;
        let mut max_camera_gap = 0.0_f32;
        let mut offroad_frames = 0;
        let mut corner_speeds = Vec::new();
        for _ in 0..600 {
            let (x, z) = waypoints[next];
            let delta = Vec3::new(x - car.position.x, 0.0, z - car.position.z);
            if delta.length() < 10.0 && next + 1 < waypoints.len() {
                corner_speeds.push(car.speed);
                next += 1;
            }
            let (x, z) = waypoints[next];
            let desired_yaw = (x - car.position.x).atan2(z - car.position.z);
            let error = (desired_yaw - car.yaw)
                .sin()
                .atan2((desired_yaw - car.yaw).cos());
            let steer = (error * 2.0).clamp(-1.0, 1.0);
            let offroad = track.road_distance(car.position) > ROAD_HALF_WIDTH;
            offroad_frames += offroad as usize;
            car.update(1.0 / 60.0, 1.0, steer, false, offroad);
            track.collide(&mut car.position, &mut car.velocity);
            chase.update(&car, 1.0 / 60.0);
            max_speed = max_speed.max(car.speed);
            let gap = (chase.camera().position - car.position).xz().length();
            max_camera_gap = max_camera_gap.max(gap);
        }
        eprintln!(
            "track drive: {} corners, max {:.2} m/s, offroad {} frames, max camera gap {:.2} m; corner speeds {corner_speeds:?}",
            corner_speeds.len(),
            max_speed,
            offroad_frames,
            max_camera_gap
        );
        assert!(corner_speeds.len() >= 4);
        assert!(max_speed > 26.0);
        assert!(offroad_frames < 120);
        assert!(max_camera_gap < 8.0);
    }
}
