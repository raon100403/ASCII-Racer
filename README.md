# ASCII Racer

A playable third-person car racing game drawn directly into terminal character cells. Its CPU renderer projects triangles, clips them at the near plane, rasterizes them into a character-and-depth framebuffer, lights flat surfaces with a directional light, and writes each completed frame to the terminal. It does not create an image framebuffer or use a GPU graphics library.

## Run

Install Rust, then run in a terminal that supports ANSI escape sequences and true color:

```sh
cargo run --release
```

A window around 100 × 34 characters or larger gives a clear view. The game adapts to terminal resizing. Use `cargo run --release -- --cube` to view the rotating cube renderer check.

## Controls

| Key | Action |
| --- | --- |
| W / Up | Accelerate |
| S / Down | Brake, then reverse |
| A / Left | Steer left |
| D / Right | Steer right |
| Space | Handbrake and drift |
| R | Reset car and checkpoint progress |
| Q / Esc | Quit |

## Game

Drive the loop and pass the cyan checkpoint gates in order. The HUD shows speed, lap, elapsed time, and the next checkpoint. Complete three laps to show `FINISH!`. Leaving the road adds heavy drag and limits speed. Guardrails on several corners push the car back and reduce its velocity on impact. The camera follows and smooths behind the car.

## Architecture

- `glam`: vectors and transforms for meshes, car, and camera.
- `crossterm`: raw terminal mode, keyboard events, terminal size, and screen control.
- `src/renderer.rs`: world-to-camera transform, perspective projection with terminal-cell aspect correction, backface culling, near-plane clipping, triangle rasterization, reciprocal-depth Z-buffer, flat directional lighting, and ASCII palette selection.
- `src/mesh.rs`: triangle and primitive box meshes.
- `src/car.rs`: arcade acceleration, reverse, steering, friction, drifting, and the car mesh.
- `src/track.rs`: generated road, ground, guardrails, checkpoint gates, off-road measurement, collision, and ordered lap progress.
- `src/main.rs`: input, chase camera, HUD, frame timing, and terminal lifetime.

The renderer's framebuffer has one cell per terminal character. Each cell stores its final character, depth, and foreground color. A full frame is assembled as one string and sent to stdout in one write, using cursor-home to overwrite the previous frame. An RAII terminal guard restores raw mode, cursor visibility, and the previous screen on exit and during panic unwinding.

## Current scope and future extensions

The track, car, and walls use code-generated geometry. Physics and collisions are intentionally simple. Possible later additions include additional tracks, AI opponents, split times, and configurable cell aspect ratio. No textures, GPU rendering, or game engine are used.
