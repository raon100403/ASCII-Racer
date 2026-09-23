//! A tiny, dependency-free ABI used by `web/index.html`.
//!
//! Keeping DOM and canvas work in JavaScript lets the Rust game compile to a
//! standalone `.wasm` without wasm-bindgen or a generated JavaScript wrapper.

use crate::{
    car::Car,
    renderer::{Camera, Renderer},
    track::{ROAD_HALF_WIDTH, Track},
};
use glam::Vec3;
use std::cell::RefCell;

const TOTAL_LAPS: u32 = 3;
const FORWARD: u32 = 1 << 0;
const BACK: u32 = 1 << 1;
const LEFT: u32 = 1 << 2;
const RIGHT: u32 = 1 << 3;
const HANDBRAKE: u32 = 1 << 4;

thread_local! {
    static GAME: RefCell<Option<WebGame>> = const { RefCell::new(None) };
}

struct WebGame {
    renderer: Renderer,
    car: Car,
    track: Track,
    camera: Camera,
    input: u32,
    elapsed: f32,
    packed_cells: Vec<u32>,
}

impl WebGame {
    fn new(width: usize, height: usize) -> Self {
        Self {
            renderer: Renderer::new(width.max(1), height.max(1), true),
            car: Car::new(),
            track: Track::new(),
            camera: Camera {
                position: Vec3::new(0.0, 3.7, -6.3),
                target: Vec3::new(0.0, 0.8, 3.0),
            },
            input: 0,
            elapsed: 0.0,
            packed_cells: Vec::with_capacity(width * height),
        }
    }

    fn tick(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.05);
        self.elapsed += dt;
        let pressed = |flag| (self.input & flag) != 0;
        let throttle = pressed(FORWARD) as i32 as f32 - pressed(BACK) as i32 as f32;
        let steer = pressed(RIGHT) as i32 as f32 - pressed(LEFT) as i32 as f32;
        let offroad = self.track.road_distance(self.car.position) > ROAD_HALF_WIDTH;
        self.car
            .update(dt, throttle, steer, pressed(HANDBRAKE), offroad);
        self.track
            .collide(&mut self.car.position, &mut self.car.velocity);
        self.track.update_checkpoint(self.car.position);

        let forward = self.car.forward();
        let desired = self.car.position - forward * 6.3 + Vec3::Y * 3.7;
        let target = self.car.position + forward * 3.0 + Vec3::Y * 0.8;
        let smoothing = 1.0 - (-5.0 * dt).exp();
        self.camera.position = self.camera.position.lerp(desired, smoothing);
        self.camera.target = self.camera.target.lerp(target, smoothing);

        self.renderer.clear();
        self.renderer.draw_mesh(&self.track.mesh, self.camera);
        self.renderer.draw_mesh(&self.car.mesh(), self.camera);
        self.draw_hud(offroad);
        self.renderer.write_packed_cells(&mut self.packed_cells);
    }

    fn draw_hud(&mut self, offroad: bool) {
        let total_secs = self.elapsed as u64;
        let status = if self.track.laps >= TOTAL_LAPS {
            "FINISH!"
        } else if offroad {
            "OFF ROAD"
        } else {
            "RACE"
        };
        self.renderer.text(
            1,
            0,
            &format!(
                "SPEED {:>3} km/h   LAP {}/{}   TIME {:02}:{:02}.{:02}   {}",
                (self.car.speed * 3.6).round() as u32,
                (self.track.laps + 1).min(TOTAL_LAPS),
                TOTAL_LAPS,
                total_secs / 60,
                total_secs % 60,
                ((self.elapsed.fract() * 100.0) as u32).min(99),
                status
            ),
        );
        self.renderer.text(
            1,
            1,
            &format!(
                "CHECKPOINT {}/{}   W/UP accelerate  S/DOWN brake  A/D steer  SPACE drift  R reset",
                self.track.checkpoint_display(),
                self.track.checkpoint_count()
            ),
        );
    }

    fn reset(&mut self) {
        self.car.reset();
        self.track.reset_progress();
        self.elapsed = 0.0;
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn game_init(width: u32, height: u32) {
    GAME.with_borrow_mut(|game| *game = Some(WebGame::new(width as usize, height as usize)));
}

#[unsafe(no_mangle)]
pub extern "C" fn game_resize(width: u32, height: u32) {
    GAME.with_borrow_mut(|game| {
        if let Some(game) = game {
            game.renderer = Renderer::new(width.max(1) as usize, height.max(1) as usize, true);
            game.packed_cells = Vec::with_capacity(width as usize * height as usize);
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn game_set_input(input: u32) {
    GAME.with_borrow_mut(|game| {
        if let Some(game) = game {
            game.input = input;
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn game_reset() {
    GAME.with_borrow_mut(|game| {
        if let Some(game) = game {
            game.reset();
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn game_tick(dt: f32) {
    GAME.with_borrow_mut(|game| {
        if let Some(game) = game {
            game.tick(dt);
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn game_frame_ptr() -> *const u32 {
    GAME.with_borrow(|game| {
        game.as_ref()
            .map_or(std::ptr::null(), |game| game.packed_cells.as_ptr())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn game_frame_len() -> usize {
    GAME.with_borrow(|game| game.as_ref().map_or(0, |game| game.packed_cells.len()))
}
