//! A tiny, dependency-free ABI used by `web/index.html`.
//!
//! Keeping DOM and canvas work in JavaScript lets the Rust game compile to a
//! standalone `.wasm` without wasm-bindgen or a generated JavaScript wrapper.

use crate::{
    car::Car,
    chase_camera::ChaseCamera,
    renderer::{GlyphMode, Renderer},
    track::{ROAD_HALF_WIDTH, Track},
};
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
    camera: ChaseCamera,
    input: u32,
    joystick_steer: f32,
    elapsed: f32,
    packed_cells: Vec<u32>,
}

impl WebGame {
    fn new(width: usize, height: usize) -> Self {
        let car = Car::new();
        let camera = ChaseCamera::new(&car);
        Self {
            renderer: Renderer::new(width.max(1), height.max(1), true),
            car,
            track: Track::new(),
            camera,
            input: 0,
            joystick_steer: 0.0,
            elapsed: 0.0,
            packed_cells: Vec::with_capacity(width * height),
        }
    }

    fn tick(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.05);
        self.elapsed += dt;
        let pressed = |flag| (self.input & flag) != 0;
        let throttle = pressed(FORWARD) as i32 as f32 - pressed(BACK) as i32 as f32;
        let digital_steer = pressed(RIGHT) as i32 as f32 - pressed(LEFT) as i32 as f32;
        let steer = if digital_steer == 0.0 {
            self.joystick_steer
        } else {
            digital_steer
        };
        let offroad = self.track.road_distance(self.car.position) > ROAD_HALF_WIDTH;
        self.car
            .update(dt, throttle, steer, pressed(HANDBRAKE), offroad);
        self.track
            .collide(&mut self.car.position, &mut self.car.velocity);
        self.track.update_checkpoint(self.car.position);

        self.camera.update(&self.car, dt);
        let camera = self.camera.camera();

        self.renderer.clear();
        self.renderer.draw_mesh(&self.track.mesh, camera);
        self.renderer.draw_ribbons(&self.track.ribbons, camera);
        self.renderer.draw_mesh(&self.car.mesh(), camera);
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
        self.camera = ChaseCamera::new(&self.car);
        self.track.reset_progress();
        self.elapsed = 0.0;
        self.renderer.reset_glyph_history();
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
            game.renderer
                .resize(width.max(1) as usize, height.max(1) as usize);
            game.packed_cells.clear();
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
pub extern "C" fn game_set_joystick_steer(steer: f32) {
    GAME.with_borrow_mut(|game| {
        if let Some(game) = game {
            game.joystick_steer = if steer.is_finite() {
                steer.clamp(-1.0, 1.0)
            } else {
                0.0
            };
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn game_set_glyph_mode(mode: u32) {
    GAME.with_borrow_mut(|game| {
        if let Some(game) = game {
            game.renderer.set_glyph_mode(if mode == 1 {
                GlyphMode::Density
            } else {
                GlyphMode::Shape
            });
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
