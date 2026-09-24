# ASCII Racer

A playable third-person car racing game drawn directly into terminal character cells. Its CPU renderer projects triangles, clips them at the near plane, rasterizes them into a character-and-depth framebuffer, lights flat surfaces with a directional light, and writes each completed frame to the terminal. It does not create an image framebuffer or use a GPU graphics library.

## Run

Install Rust, then run in a terminal that supports ANSI escape sequences and true color:

```sh
cargo run --release
```

A window around 100 × 34 characters or larger gives a clear view. The game adapts to terminal resizing. Use `cargo run --release -- --cube` to view the rotating cube renderer check. The default four depth-tested samples per character soften thin edges; `--aa off` restores single center sampling for comparison, and `--aa 2x2` selects the default explicitly. Adjust font proportions with `--cell-aspect 0.5` (positive cell width / cell height; default `0.5`). For example: `cargo run --release -- --cell-aspect 0.6 --aa 2x2`.

## WebAssembly / browser

Install the Rust WASM target once, build the browser version, then serve the
`web` directory over HTTP:

```sh
rustup target add wasm32-unknown-unknown
./build-web.sh
python3 -m http.server 8000 --directory web
```

Open <http://localhost:8000>. The browser build uses the same Rust physics,
track, camera, and ASCII renderer as the terminal version. `web/index.html`
provides the canvas display, keyboard input, and touch controls; the build
places the standalone module at `web/ascii_racer.wasm`.

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
- `src/renderer.rs`: world-to-camera transform, perspective projection with configurable terminal-cell aspect correction, backface culling, near-plane clipping, four depth-tested samples per character, coverage-aware ASCII resolve, RGB directional lighting, and reciprocal-depth Z-buffer.
- `src/mesh.rs`: triangles tagged with small surface classifications and primitive box meshes.
- `src/car.rs`: arcade acceleration, reverse, steering, friction, drifting, and the car mesh.
- `src/track.rs`: generated road, ground, guardrails, checkpoint gates, off-road measurement, collision, and ordered lap progress.
- `src/main.rs`: input, chase camera, HUD, frame timing, and terminal lifetime.

The renderer rasterizes triangles directly at four sub-cell positions per terminal character (one in `--aa off` mode); it does not create an image framebuffer. Each sample retains its own depth, color, lighting, and surface classification. Distant road paint that falls between all four samples gets a conservative fractional cell-overlap contribution, still depth-tested and rendered with a light glyph. Resolve combines visible coverage and surface importance into one ASCII glyph and lit foreground color per cell. HUD text overlays the scene. A full frame is assembled as one string and sent to stdout in one write, using cursor-home to overwrite the previous frame. The browser consumes the same resolved cells through `write_packed_cells()`. An RAII terminal guard restores raw mode, cursor visibility, and the previous screen on exit and during panic unwinding.

## Current scope and future extensions

The track, car, and walls use code-generated geometry. Physics and collisions are intentionally simple. Possible later additions include additional tracks, AI opponents, and split times. No textures, GPU rendering, or game engine are used.
