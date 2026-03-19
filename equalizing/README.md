# equalizing

Integration test suite and dev environment for verifying **output parity** between [matrix-commander](https://github.com/8go/matrix-commander) (Python) and [matrix-commander-ng](https://github.com/longregen/matrix-commander-ng) (Rust).

The goal is to ensure the Rust port produces identical (or equivalent) output for every CLI operation the Python version supports — credential formats, JSON output shapes, text formatting, and behavioral semantics.

## Structure

```
equalizing/
├── flake.nix                  # Nix flake: dev shell, sandbox, NixOS VM test
├── flake.lock
└── tests/
    ├── nixos-test.nix          # NixOS VM test: runs both CLIs side-by-side and compares output
    ├── test-sandbox.sh          # CLI + infra smoke tests inside the bubblewrap sandbox
    ├── test-browser.py          # Browser integration tests (Element Web + Cinny via Playwright)
    └── test-all.sh              # Combined runner: sandbox tests then browser tests
```

## Prerequisites

- Nix with flakes enabled

## Quick start

### Dev shell

```sh
cd equalizing
nix develop
```

This drops you into a shell with all dependencies available (Python + matrix-commander, Rust toolchain, Synapse, PostgreSQL, nginx, Element Web, Cinny, Playwright) and provides helper functions:

| Command | Description |
|---|---|
| `start-all` | Start PostgreSQL + Synapse + nginx (Element Web + Cinny) |
| `stop-all` | Stop everything |
| `clean-all` | Stop everything and delete all dev data |
| `register-user <name> [password]` | Register a Matrix test user |
| `register-admin <name> [password]` | Register a Matrix admin user |
| `build-ng` | Build matrix-commander-ng from the local source |
| `sandbox` | Enter an isolated bubblewrap sandbox (fish shell) |

### Bubblewrap sandbox

The sandbox provides **network and PID isolation** via bubblewrap — all processes are killed on exit and only localhost networking is available. Useful for running tests without affecting the host system.

```sh
# From the dev shell:
sandbox

# Or directly:
nix run .#sandbox
```

The sandbox uses a fish shell with the same helper functions listed above.

### Running tests

**Sandbox + browser tests** (inside the sandbox):
```sh
nix run .#sandbox -- --run bash tests/test-all.sh
```

**Sandbox tests only**:
```sh
nix run .#sandbox -- --run bash tests/test-sandbox.sh
```

**Browser tests only** (requires Synapse already running):
```sh
nix run .#sandbox -- --run python tests/test-browser.py
```

**NixOS VM tests** (full isolation, no sandbox needed):
```sh
nix build .#checks.x86_64-linux.integration-test -L
```

## What the tests cover

### `test-sandbox.sh` — Infrastructure and CLI smoke tests

- Tool availability (cargo, rustc, synapse, pg_ctl, etc.)
- Python imports (nio, aiohttp, PIL, etc.)
- PostgreSQL init and connectivity
- Synapse startup and API responsiveness
- User registration and authentication
- Room creation, joining, messaging via the Matrix client API
- Media upload/download round-trips (image and audio) with content verification
- Room message history validation
- Rust build environment (openssl, sqlite3 pkg-config)

### `test-browser.py` — Browser integration tests

Tests Element Web and Cinny (served via nginx) against the local Synapse, using Playwright to verify that messages sent via matrix-commander CLIs are visible in web clients and vice versa.

### `nixos-test.nix` — Output parity VM test

Spins up a NixOS VM with Synapse and runs a 65-test suite against both `matrix-commander` (Python) and `matrix-commander-ng`, then compares:

- **Credential format**: matching JSON keys, homeserver URL format, refresh_token presence
- **`--joined-rooms`**: outputs room IDs only (no `Room:` prefix)
- **`--devices`**: tabular format (no `Device:` prefix), JSON output with `device_id`, `display_name`, `last_seen_ip`, `last_seen_ts`
- **`--print-event-id`**: `$event_id    !room_id    message` format with matching field count
- **`--room-create --output json`**: includes `alias_full`
- **`--get-room-info --output json`**: includes `room_id`, `display_name`, `alias`, `topic`, `encrypted`
- **`--joined-members --output json`**: `{room_id, members: [{user_id, display_name, avatar_url}]}`
- **`--login-info`**: both show `m.login.password`

Individual CLI operations tested: login (multi-device), version, whoami, room create, send message, list devices, get/set display name, joined rooms, device verification, invite, join, listen tail, discovery info, login info, content repository config.

## Dev data

All runtime state (PostgreSQL data, Synapse config/media, cargo cache, nginx logs) is stored under `.dev-data/` relative to the project root. Run `clean-all` to wipe it.
