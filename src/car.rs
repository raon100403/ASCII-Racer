use crate::mesh::Mesh;
use glam::{Mat4, Vec3};

const MAX_SPEED: f32 = 34.0;
const MAX_REVERSE: f32 = 10.0;

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
        self.yaw +=
            self.steering * direction * turn_strength * (if handbrake { 2.2 } else { 1.65 }) * dt;
        let f = self.forward();
        let right = Vec3::new(f.z, 0.0, -f.x);
        let acceleration = if throttle > 0.0 {
            22.0
        } else if forward_speed > 1.0 {
            32.0
        } else {
            13.0
        };
        self.velocity += f * throttle * acceleration * dt;
        let longitudinal = self.velocity.dot(f);
        let lateral = self.velocity.dot(right);
        let rolling_drag = if offroad {
            2.8
        } else if handbrake {
            1.1
        } else {
            0.46
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
        let limit = if offroad { 13.0 } else { MAX_SPEED };
        let forward_speed = self.velocity.dot(f).clamp(-MAX_REVERSE, limit);
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
}
