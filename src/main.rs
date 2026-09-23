#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

#[cfg(not(target_arch = "wasm32"))]
use ascii_racer::{
    car::Car,
    mesh::Mesh,
    renderer::{Camera, Renderer},
    track::{ROAD_HALF_WIDTH, Track},
};
#[cfg(not(target_arch = "wasm32"))]
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind},
    execute, terminal,
};
#[cfg(not(target_arch = "wasm32"))]
use glam::{Mat4, Vec3};
#[cfg(not(target_arch = "wasm32"))]
use std::io::{self, Write};
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};

#[cfg(not(target_arch = "wasm32"))]
const FRAME_TIME: Duration = Duration::from_millis(33);
#[cfg(not(target_arch = "wasm32"))]
const INPUT_HOLD: Duration = Duration::from_millis(250);
#[cfg(not(target_arch = "wasm32"))]
const TOTAL_LAPS: u32 = 3;

#[cfg(not(target_arch = "wasm32"))]
struct Terminal;
#[cfg(not(target_arch = "wasm32"))]
impl Terminal {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        if let Err(error) = execute!(io::stdout(), terminal::EnterAlternateScreen, cursor::Hide) {
            let _ = terminal::disable_raw_mode();
            return Err(error);
        }
        Ok(Self)
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), cursor::Show, terminal::LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct Inputs {
    forward: Option<Instant>,
    back: Option<Instant>,
    left: Option<Instant>,
    right: Option<Instant>,
    handbrake: Option<Instant>,
    reset: bool,
    quit: bool,
}
#[cfg(not(target_arch = "wasm32"))]
impl Inputs {
    fn handle(&mut self, event: Event) {
        if let Event::Key(key) = event {
            let code = match key.code {
                KeyCode::Char(c) => KeyCode::Char(c.to_ascii_lowercase()),
                other => other,
            };
            if key.kind == KeyEventKind::Press && matches!(code, KeyCode::Esc | KeyCode::Char('q'))
            {
                self.quit = true;
            }
            if key.kind == KeyEventKind::Press && code == KeyCode::Char('r') {
                self.reset = true;
            }
            let slot = match code {
                KeyCode::Up | KeyCode::Char('w') => Some(&mut self.forward),
                KeyCode::Down | KeyCode::Char('s') => Some(&mut self.back),
                KeyCode::Left | KeyCode::Char('a') => Some(&mut self.left),
                KeyCode::Right | KeyCode::Char('d') => Some(&mut self.right),
                KeyCode::Char(' ') => Some(&mut self.handbrake),
                _ => None,
            };
            if let Some(slot) = slot {
                match key.kind {
                    KeyEventKind::Release => *slot = None,
                    KeyEventKind::Press | KeyEventKind::Repeat => *slot = Some(Instant::now()),
                }
            }
        }
    }
    fn held(value: Option<Instant>) -> bool {
        value.is_some_and(|t| t.elapsed() < INPUT_HOLD)
    }
    fn throttle(&self) -> f32 {
        Self::held(self.forward) as i32 as f32 - Self::held(self.back) as i32 as f32
    }
    fn steer(&self) -> f32 {
        Self::held(self.right) as i32 as f32 - Self::held(self.left) as i32 as f32
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> io::Result<()> {
    // The guard is dropped during normal return and panic unwinding.
    let _terminal = Terminal::enter()?;
    let cube_demo = std::env::args().any(|arg| arg == "--cube");
    let (w, h) = terminal::size()?;
    let mut renderer = Renderer::new(
        w.saturating_sub(1).max(1) as usize,
        h.saturating_sub(1).max(1) as usize,
        true,
    );
    let mut inputs = Inputs::default();
    let mut car = Car::new();
    let mut track = Track::new();
    let mut camera = Camera {
        position: Vec3::new(0.0, 3.7, -6.3),
        target: Vec3::new(0.0, 0.8, 3.0),
    };
    let cube = Mesh::box_mesh(Vec3::splat(2.0), (255, 190, 80));
    let start = Instant::now();
    let mut last = start;
    loop {
        let frame_start = Instant::now();
        let dt = frame_start.duration_since(last).as_secs_f32().min(0.05);
        last = frame_start;
        while event::poll(Duration::ZERO)? {
            inputs.handle(event::read()?);
        }
        if inputs.quit {
            break;
        }
        let (w, h) = terminal::size()?;
        let new_w = w.saturating_sub(1).max(1) as usize;
        let new_h = h.saturating_sub(1).max(1) as usize;
        if new_w != renderer.width || new_h != renderer.height {
            renderer = Renderer::new(new_w, new_h, true);
        }
        renderer.clear();
        if cube_demo {
            let mut scene = Mesh::new();
            scene.append_transformed(
                &cube,
                Mat4::from_rotation_y(start.elapsed().as_secs_f32() * 0.7),
            );
            renderer.draw_mesh(
                &scene,
                Camera {
                    position: Vec3::new(0.0, 2.0, -7.0),
                    target: Vec3::ZERO,
                },
            );
            renderer.text(1, 0, "RENDERER CHECK: rotating cube  |  Q / Esc quit");
        } else {
            if inputs.reset {
                car.reset();
                track.reset_progress();
                inputs.reset = false;
            }
            let offroad = track.road_distance(car.position) > ROAD_HALF_WIDTH;
            car.update(
                dt,
                inputs.throttle(),
                inputs.steer(),
                Inputs::held(inputs.handbrake),
                offroad,
            );
            track.collide(&mut car.position, &mut car.velocity);
            track.update_checkpoint(car.position);
            let forward = car.forward();
            let desired = car.position - forward * 6.3 + Vec3::Y * 3.7;
            let target = car.position + forward * 3.0 + Vec3::Y * 0.8;
            let smoothing = 1.0 - (-5.0 * dt).exp();
            camera.position = camera.position.lerp(desired, smoothing);
            camera.target = camera.target.lerp(target, smoothing);
            renderer.draw_mesh(&track.mesh, camera);
            renderer.draw_mesh(&car.mesh(), camera);
            draw_hud(&mut renderer, &car, &track, start.elapsed(), offroad);
        }
        let output = renderer.frame_string();
        let mut stdout = io::stdout().lock();
        stdout.write_all(output.as_bytes())?;
        stdout.flush()?;
        std::thread::sleep(FRAME_TIME.saturating_sub(frame_start.elapsed()));
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
fn draw_hud(renderer: &mut Renderer, car: &Car, track: &Track, elapsed: Duration, offroad: bool) {
    let total_secs = elapsed.as_secs();
    let status = if track.laps >= TOTAL_LAPS {
        "FINISH!"
    } else if offroad {
        "OFF ROAD"
    } else {
        "RACE"
    };
    renderer.text(
        1,
        0,
        &format!(
            "SPEED {:>3} km/h   LAP {}/{}   TIME {:02}:{:02}.{:02}   {}",
            (car.speed * 3.6).round() as u32,
            (track.laps + 1).min(TOTAL_LAPS),
            TOTAL_LAPS,
            total_secs / 60,
            total_secs % 60,
            elapsed.subsec_millis() / 10,
            status
        ),
    );
    renderer.text(1, 1, &format!("CHECKPOINT {}/{}   W/UP accelerate  S/DOWN brake  A/D steer  SPACE drift  R reset  Q quit",
        track.checkpoint_display(), track.checkpoint_count()));
}
