use crate::mesh::{Mesh, SurfaceKind};
use glam::{Mat4, Vec3};

pub const MAX_FORWARD_SPEED: f32 = 27.0;

struct DrivingTuning {
    engine_force: f32,
    brake_force: f32,
    reverse_force: f32,
    max_reverse_speed: f32,
    road_drag: f32,
    handbrake_drag: f32,
    offroad_drag: f32,
    offroad_speed: f32,
    steering_rate: f32,
    handbrake_steering_rate: f32,
    high_speed_steer_start: f32,
    high_speed_steer_reduction: f32,
}

const DRIVING: DrivingTuning = DrivingTuning {
    engine_force: 19.5,
    brake_force: 28.0,
    reverse_force: 13.0,
    max_reverse_speed: 10.0,
    road_drag: 0.46,
    handbrake_drag: 1.1,
    offroad_drag: 2.8,
    offroad_speed: 13.0,
    steering_rate: 1.65,
    handbrake_steering_rate: 2.2,
    high_speed_steer_start: 0.55,
    high_speed_steer_reduction: 0.10,
};

pub struct Car {
    pub position: Vec3,
    pub yaw: f32,
    pub velocity: Vec3,
    pub speed: f32,
    pub steering: f32,
}

impl Car {
    pub fn new() -> Self {
        Self {
            position: Vec3::new(0.0, 0.0, 1.0),
            yaw: 0.0,
            velocity: Vec3::ZERO,
            speed: 0.0,
            steering: 0.0,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn forward(&self) -> Vec3 {
        Vec3::new(self.yaw.sin(), 0.0, self.yaw.cos())
    }

    pub fn update(&mut self, dt: f32, throttle: f32, steer: f32, handbrake: bool, offroad: bool) {
        let dt = dt.min(0.05);
        let f = self.forward();
        let forward_speed = self.velocity.dot(f);
        self.steering += (steer - self.steering) * (1.0 - (-9.0 * dt).exp());
        let turn_strength = (forward_speed.abs() / 9.0).clamp(0.15, 1.0);
        let direction = if forward_speed < -0.5 { -1.0 } else { 1.0 };
        let high_speed = ((forward_speed.abs() / MAX_FORWARD_SPEED
            - DRIVING.high_speed_steer_start)
            / (1.0 - DRIVING.high_speed_steer_start))
            .clamp(0.0, 1.0);
        let high_speed = high_speed * high_speed * (3.0 - 2.0 * high_speed);
        let steering_rate = if handbrake {
            DRIVING.handbrake_steering_rate
        } else {
            DRIVING.steering_rate
        };
        self.yaw += self.steering
            * direction
            * turn_strength
            * steering_rate
            * (1.0 - DRIVING.high_speed_steer_reduction * high_speed)
            * dt;
        let f = self.forward();
        let right = Vec3::new(f.z, 0.0, -f.x);
        let acceleration = if throttle > 0.0 {
            // Balance engine force with road drag at the desired terminal speed.
            // The quadratic falloff retains low-speed response and avoids
            // spending the whole straight against the safety speed clamp.
            let ratio = (forward_speed.max(0.0) / MAX_FORWARD_SPEED).clamp(0.0, 1.0);
            let falloff = 1.0 - DRIVING.road_drag * MAX_FORWARD_SPEED / DRIVING.engine_force;
            DRIVING.engine_force * (1.0 - falloff * ratio * ratio)
        } else if forward_speed > 1.0 {
            DRIVING.brake_force
        } else {
            DRIVING.reverse_force
        };
        self.velocity += f * throttle * acceleration * dt;
        let longitudinal = self.velocity.dot(f);
        let lateral = self.velocity.dot(right);
        let rolling_drag = if offroad {
            DRIVING.offroad_drag
        } else if handbrake {
            DRIVING.handbrake_drag
        } else {
            DRIVING.road_drag
        };
        let lateral_grip = if handbrake {
            0.8
        } else if offroad {
            2.7
        } else {
            7.0
        };
        self.velocity = f * longitudinal * (-rolling_drag * dt).exp()
            + right * lateral * (-lateral_grip * dt).exp();
        let limit = if offroad {
            DRIVING.offroad_speed
        } else {
            MAX_FORWARD_SPEED
        };
        let forward_speed = self
            .velocity
            .dot(f)
            .clamp(-DRIVING.max_reverse_speed, limit);
        self.velocity = f * forward_speed + right * self.velocity.dot(right);
        self.position += self.velocity * dt;
        self.speed = self.velocity.length();
    }

    pub fn mesh(&self) -> Mesh {
        let mut local = Mesh::new();
        add_box(
            &mut local,
            Vec3::new(0.0, 0.53, 0.0),
            Vec3::new(1.55, 0.55, 3.3),
            (245, 65, 52),
        );
        add_box(
            &mut local,
            Vec3::new(0.0, 1.02, -0.25),
            Vec3::new(1.25, 0.57, 1.7),
            (242, 112, 66),
        );
        add_box(
            &mut local,
            Vec3::new(0.0, 1.04, 0.63),
            Vec3::new(1.16, 0.4, 0.14),
            (85, 192, 218),
        );
        for x in [-0.83, 0.83] {
            for z in [-1.05, 1.05] {
                add_box(
                    &mut local,
                    Vec3::new(x, 0.34, z),
                    Vec3::new(0.28, 0.65, 0.56),
                    (42, 45, 52),
                );
            }
        }
        for x in [-0.53, 0.53] {
            add_box(
                &mut local,
                Vec3::new(x, 0.57, 1.68),
                Vec3::new(0.32, 0.16, 0.07),
                (255, 236, 145),
            );
        }
        local = local.with_surface(SurfaceKind::Vehicle);
        let transform = Mat4::from_translation(self.position) * Mat4::from_rotation_y(self.yaw);
        let mut world = Mesh::new();
        world.append_transformed(&local, transform);
        world
    }
}

fn add_box(mesh: &mut Mesh, center: Vec3, size: Vec3, color: (u8, u8, u8)) {
    mesh.append_transformed(&Mesh::box_mesh(size, color), Mat4::from_translation(center));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accelerating_and_steering_moves_car() {
        let mut car = Car::new();
        for _ in 0..60 {
            car.update(1.0 / 60.0, 1.0, -1.0, false, false);
        }
        assert!(car.position.z > 2.0);
        assert!(car.position.x < 0.0);
        assert!(car.speed > 1.0);
    }

    #[test]
    fn forward_build_is_responsive_and_approaches_a_soft_terminal_speed() {
        let mut car = Car::new();
        for frame in 0..1200 {
            car.update(1.0 / 60.0, 1.0, 0.0, false, false);
            if frame == 59 {
                assert!((14.0..16.0).contains(&car.speed));
            }
            if frame == 119 {
                assert!((21.0..23.0).contains(&car.speed));
            }
        }
        assert!((26.8..=MAX_FORWARD_SPEED).contains(&car.speed));
    }

    #[test]
    fn braking_distance_and_reverse_speed_match_the_lower_top_speed() {
        let mut car = Car::new();
        car.velocity = car.forward() * MAX_FORWARD_SPEED;
        let start = car.position;
        let mut stop_time = None;
        for frame in 1..=120 {
            car.update(1.0 / 60.0, -1.0, 0.0, false, false);
            if car.velocity.dot(car.forward()) <= 0.0 {
                stop_time = Some(frame as f32 / 60.0);
                break;
            }
        }
        let distance = car.position.distance(start);
        assert!((0.7..0.9).contains(&stop_time.unwrap()));
        assert!((9.0..11.0).contains(&distance), "{distance}");
        for _ in 0..600 {
            car.update(1.0 / 60.0, -1.0, 0.0, false, false);
        }
        assert!((-10.0..-9.8).contains(&car.velocity.dot(car.forward())));
    }

    #[test]
    fn high_speed_steering_is_controlled_without_weakening_low_speed_steering() {
        let yaw_in_one_step = |speed: f32| {
            let mut car = Car::new();
            car.velocity = car.forward() * speed;
            car.steering = 1.0;
            car.update(1.0 / 60.0, 0.0, 1.0, false, false);
            car.yaw * 60.0
        };
        let low = yaw_in_one_step(10.0);
        let high = yaw_in_one_step(MAX_FORWARD_SPEED);
        assert!((1.6..1.7).contains(&low));
        assert!((1.45..1.55).contains(&high));
    }

    #[test]
    fn driving_remains_nearly_independent_of_frame_rate() {
        let simulate = |dt: f32, frames: usize| {
            let mut car = Car::new();
            for _ in 0..frames {
                car.update(dt, 1.0, 0.0, false, false);
            }
            car
        };
        let fast = simulate(1.0 / 60.0, 300);
        let slow = simulate(1.0 / 30.0, 150);
        assert!((fast.speed - slow.speed).abs() < 0.1);
        assert!(fast.position.distance(slow.position) < 0.5);
    }
}
