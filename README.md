# lnp2


Rust PnP machine controller for LumenPnP. Replaces OpenPnP with a single binary that owns serial, motion, actuators, and serves an HTTP API.

## Quick Start

```bash
cargo build --release
./target/release/rsrvpnp config/machine.toml
```

The server starts on `0.0.0.0:3000`.

## API

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
| GET | `/api/vacuum/read` | Read vacuum sensor (nozzle) |
| POST | `/api/blow` | Blow-off pulse (nozzle, duration_ms) |
| POST | `/api/led` | LED control (r, g, b, brightness, off) |
| GET | `/api/config` | Get machine config |
| PUT | `/api/config` | Update machine config |

## Configuration

All machine parameters are in `config/machine.toml`: serial port, axis limits, nozzle commands, camera settings, LED commands.
