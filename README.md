# lnp2

    Look N Pick N Look N Place

Single-binary pick-and-place machine controller for LumenPnP. Replaces OpenPnP with a Rust backend and Svelte frontend — owns serial, motion, cameras, vision, feeders, and serves everything over HTTP.

## Quick Start

```bash
# Build
cargo build --release
cd web && npm install && npx vite build && cd ..

# Run
./target/release/lnp2 serve --config config/
```

Server starts on `0.0.0.0:3000` — open in a browser for the control UI.

## Architecture

```
lnp2 binary
├── Serial (GCode)  ─────→  Marlin firmware (Rambo board)
│   ├── Motion         5-axis: XY gantry, Z, A/B rotation
│   ├── Actuators      Vacuum, blow-off, LED, I2C sensors
│   └── RS-485 bus     Photon Smart Feeder protocol
│
├── Cameras (V4L2)  ─────→  Top (down-looking) + Bottom (up-looking)
│   ├── MJPEG capture    15fps, turbojpeg lossless flip
│   ├── Auto-exposure    v4l2 brightness tracking
│   └── WebSocket stream Binary frames to browser
│
├── Vision  ─────────────→  OpenCV + ONNX Runtime (YOLOv8)
│   ├── Pocket detection   Locate feeder slots (ML or CV fallback)
│   ├── Fiducial detection PCB alignment markers
│   └── Part alignment     Pad-level rotation correction
│
└── Web UI (Svelte)  ────→  Served from web/dist/
    ├── Live camera view   Canvas + crosshair + ML overlay
    ├── Click-to-move      Pixel → mm via camera calibration
    ├── Jog controls       Step/feedrate, keyboard shortcuts
    └── Actuator panel     Vacuum, blow, LED
```

## CLI Commands

```bash
lnp2 serve --config config/          # Start server (default)
lnp2 import --from ~/.openpnp2 --to config/  # Import OpenPnP XML → TOML
lnp2 scan --config config/           # Discover feeders on RS-485 bus
lnp2 feed --config config/ --slot 3  # Test-feed a single slot
```

## API

### Motion & Actuators

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/gcode` | Send raw GCode command |
| POST | `/api/gcode/batch` | Send multiple GCode commands |
| POST | `/api/move` | Move to coordinates (x, y, z, a, b, feedrate) |
| POST | `/api/move/safe` | Safe XY move (retracts Z first) |
| POST | `/api/home` | Home all axes |
| GET | `/api/position` | Get current position |
| POST | `/api/acceleration` | Set acceleration |
| POST | `/api/vacuum` | Vacuum on/off (nozzle, action) |
| GET | `/api/vacuum/read` | Read vacuum sensor |
| POST | `/api/blow` | Blow-off pulse (nozzle, duration_ms) |
| POST | `/api/led` | LED control (r, g, b, brightness, off) |

### Camera & Vision

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/camera/list` | List cameras and configs |
| GET | `/api/camera/capture` | Single JPEG frame |
| WS | `/api/camera/stream?name=top` | Live MJPEG WebSocket stream |
| POST | `/api/vision/detect_all` | Run YOLOv8 inference, return detections |
| POST | `/api/vision/detect_pocket` | Detect feeder pocket, return mm offset |
| POST | `/api/vision/detect_fiducial` | Detect PCB fiducial marker |
| POST | `/api/vision/align_part` | Pad-level part alignment |

### Config & Data

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/config` | Get machine config |
| PUT | `/api/config` | Update machine config (hot-reload) |
| POST | `/api/dataset/capture` | Save labeled frame for ML training |
| GET | `/api/dataset/count` | Count captured images |

### WebSocket Events

`/api/events` streams JSON position updates and state changes:
```json
{"type": "Position", "x": 10.5, "y": 20.3, "z": 15.0, "a": 0, "b": 0}
```

## Configuration

All machine parameters live in `config/*.toml`:

| File | Contents |
|------|----------|
| `machine.toml` | Serial port, axes, nozzles, cameras, LEDs, vision models |
| `feeders.toml` | Feeder definitions (Photon slots, tray positions) |
| `parts.toml` | Part library (package, speed, value) |
| `packages.toml` | Package footprints (pads, body dimensions) |
| `nozzle_tips.toml` | Tip changer waypoint sequences |

Use `lnp2 import` to convert an existing OpenPnP configuration.

## Build Dependencies

Rust crate dependencies are handled by Cargo. System libraries needed on the target:

- `libopencv-dev` — image processing and classical CV
- `libturbojpeg0-dev` — lossless JPEG transforms
- `nasm` + `cmake` — for building turbojpeg from source
- ONNX Runtime — loaded dynamically via `ort` (set `ORT_DYLIB_PATH` if needed)

## Deploy

Deployment uses [just](https://github.com/casey/just) recipes to sync, build, and manage the service on `lumeneer.local`.

```bash
just setup       # One-time remote setup (install deps, create dirs, enable service)
just check       # Verify remote environment (toolchain, libs, disk)

just build       # Sync source + cargo build --release on remote
just build-web   # Sync source + npm install + vite build on remote
just deploy      # Build all + copy to production dir + restart systemd service
just deploy-rust # Binary-only deploy (skip web build)

just run         # Build + run server in foreground (for dev/debug)
just logs        # Follow journalctl output
just status      # Show systemd service status

just watch       # Auto-sync on file save (requires fswatch)
just ssh         # SSH into remote build dir
just builds      # List remote build dirs with disk usage
just clean       # Remove a remote build dir
```

The remote uses per-branch build dirs (`~/lnp2/builds/<branch>/`) and a single production dir (`~/lnp2/production/`) managed by a systemd service (`remote/lnp2.service`).
