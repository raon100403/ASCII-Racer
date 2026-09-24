# ASCII Racer

A playable third-person car racing game drawn directly into terminal character cells. Its CPU renderer projects triangles, clips them at the near plane, rasterizes them into a character-and-depth framebuffer, lights flat surfaces with a directional light, and writes each completed frame to the terminal. It does not create an image framebuffer or use a GPU graphics library.

## Run

Install Rust, then run in a terminal that supports ANSI escape sequences and true color:

```sh
cargo run --release
```

A window around 100 × 34 characters or larger gives a clear view. The game adapts to terminal resizing. Use `cargo run --release -- --cube` to view the rotating cube renderer check. The default four depth-tested samples per character soften thin edges; `--aa off` restores single center sampling for comparison, and `--aa 2x2` selects the default explicitly. Adjust font proportions with `--cell-aspect 0.5` (positive cell width / cell height; default `0.5`). For example: `cargo run --release -- --cell-aspect 0.6 --aa 2x2`.

Use `--debug-coverage` for unassisted ribbon AA coverage, `--debug-effective-coverage` for coverage after depth and perceptual remapping, `--debug-road-marking-width` for true projected width, and `--debug-road-marking-fade` for the distance fade. Each uses grayscale glyph intensity with a faint diagnostic floor. `--debug-road-markings` shows paint contributing to resolve (green `#`), paint lost to scoring or the far fade (yellow `.`), and paint rejected by depth (red `.`). These terminal-only views do not change browser rendering.

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
- `src/renderer.rs`: perspective projection, near/far clipping, 2×2 depth-tested MSAA for ordinary triangles, screen-space distance coverage for road ribbons, shared depth/coverage ASCII resolve, and RGB lighting.
- `src/mesh.rs`: triangle surface classifications, box meshes, and world-space road-ribbon render primitives.
- `src/car.rs`: arcade acceleration, reverse, steering, friction, drifting, and the car mesh.
- `src/track.rs`: generated road, precomputed continuous edge and dashed center ribbons, guardrails, checkpoints, off-road measurement, collision, and ordered lap progress.
- `src/main.rs`: input, chase camera, HUD, frame timing, and terminal lifetime.

The renderer rasterizes ordinary triangles at four sub-cell positions per terminal character (one in `--aa off` mode); it does not create an image framebuffer. Road markings are separate center segments with world-space width, color, and shade—never duplicate paint triangles. Ribbons are clipped at the near and far planes, projected with perspective-varying width, and tested only within a tight expanded screen-space bounding box. Closest-segment distance and an AA kernel yield **true coverage from the unassisted projected width**; thinner ribbons get slightly wider AA support near cell corners without changing their world width. Optical width and a gentle low-coverage remap supply **effective coverage only where true coverage is nonzero**, followed by one smooth far-distance fade. Foreground samples conservatively occlude paint before this visibility boost; visible road/ground and paint blend continuously rather than switching at a score tie. `Renderer::road_marking_cell(x, y)` exposes true and depth-resolved effective coverage, projected width, nearest screen point and direction, reciprocal-derived depth, surface, color, and brightness after a normal resolve for future glyph selection. Directional glyph selection is not implemented here. HUD text overlays the scene. A full frame is assembled as one string and sent to stdout in one write; the browser consumes the same resolved cells through `write_packed_cells()`. An RAII terminal guard restores terminal state on exit.

## Current scope and future extensions

The track, car, and walls use code-generated geometry. Physics and collisions are intentionally simple. Possible later additions include additional tracks, AI opponents, and split times. No textures, GPU rendering, or game engine are used.
