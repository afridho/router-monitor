# Router Monitor

An open-source, read-only router dashboard built with Rust/Axum and React/Vite. Live router telemetry, Liquid Glass UI, automatic light/dark appearance, and persistent SQLite history. No Node.js server is required at runtime.

## Tested routers

| Router | Firmware | Verification |
| --- | --- | --- |
| Huawei OptiXstar HG8145V6 | V5R023C00S200 | Login, system/optical telemetry, GPON counters, client inventory, Ethernet, and Wi-Fi association matching tested on a physical device |

The router address is configurable, but **this is not a universal router driver**. The current adapter understands specific Huawei web-page constructor schemas. Other Huawei models/firmware and other manufacturers are unverified. ISP-customized firmware and account permissions may change availability. Missing readings are not evidence of a healthy or unhealthy connection.

To report compatibility, include model, firmware, and which readings work. Never upload credentials, cookies, configuration backups, or unredacted router-page captures.

## Features

- Six monitoring visuals: CPU/memory gauges, throughput, optical power, system trends, and online-device count.
- One global 1D / 1W / 1M / 3M history selector; charts show aggregated interval averages.
- System, optical transceiver, GPON traffic/totals, Wi-Fi, and Ethernet readings.
- Device search and online/offline/band filters; MAC deduplication and expandable additional addresses.
- Custom device labels persisted in SQLite. Existing browser labels migrate automatically after successful import without overwriting existing database labels.
- Responsive three/two/one-column layouts and non-interactive device badges.
- No router configuration, reboot, DNS changes, website filtering, or counter-reset endpoints are registered.

## Build and run

Requirements: a current stable Rust toolchain supporting edition 2024, a C build toolchain for bundled SQLite, and Node.js compatible with Vite 8 (20.19+ or 22.12+). Use an actively maintained Node LTS.

```bash
git clone git@github.com:afridho/router-monitor.git
cd router-monitor
npm ci --prefix frontend
npm run build --prefix frontend
cargo build --release
ROUTER_URL=https://192.168.100.1:80 ./target/release/router-monitor
```

Open **http://127.0.0.1:8787**, then connect with your own router account. Do not assume an account name/password is universal.

Environment variables (export them explicitly; `.env` is not loaded automatically):

| Variable | Default | Meaning |
| --- | --- | --- |
| `ROUTER_URL` | `https://192.168.100.1:80` | One HTTP(S) router origin; no credentials/path/query/fragment |
| `STATIC_DIR` | `frontend/dist` | Built frontend path, relative to the process working directory or absolute |

Run from a stable application directory: `data/monitor.sqlite3` is relative to the working directory. The frontend dev server is optional (`npm run dev --prefix frontend`); the Rust backend must also be running.

## Homeserver deployment

The server must be able to reach the router's management address. Build locally on the homeserver, or copy a binary built for its CPU/OS together with `frontend/dist`. Building on another CPU architecture does not automatically produce a compatible binary.

The backend deliberately binds only to **127.0.0.1:8787** and checks Host/Origin. It has **no dashboard user authentication**. Router login is not dashboard access control: local label/history endpoints are available without router login.

For safe remote access, use an SSH tunnel from your client computer:

```bash
ssh -N -L 127.0.0.1:8787:127.0.0.1:8787 user@homeserver
```

Then visit `http://127.0.0.1:8787` on that computer. Stop a local Router Monitor instance first if the port is already occupied. Do not expose this application directly to the Internet or assume an unauthenticated reverse proxy is safe. LAN-wide hosting needs a separate authenticated deployment design, not just a bind-address change.

Example user systemd unit, `~/.config/systemd/user/router-monitor.service` (replace `/srv/router-monitor` with your actual installation path):

```ini
[Unit]
Description=Router Monitor
After=network-online.target

[Service]
WorkingDirectory=/srv/router-monitor
ExecStart=/srv/router-monitor/router-monitor
Environment=ROUTER_URL=https://192.168.100.1:80
Environment=STATIC_DIR=/srv/router-monitor/frontend/dist
Restart=on-failure
RestartSec=5
UMask=0077
NoNewPrivileges=true
PrivateTmp=true

[Install]
WantedBy=default.target
```

Copy the release binary to the `ExecStart` location and ensure this user owns the writable `data/` directory. Enable the unit with `systemctl --user enable --now router-monitor`. User lingering is needed if it must run after logout. This is a deployment example, not an installed service. Router sessions are in memory: **every process restart requires reconnecting through the UI**. No samples are collected without an active session.

## Memory and storage

Measured on Linux x86-64 with a release build on 2026-11-20. These are small local measurements, **not minimum hardware guarantees or long-running load tests**.

| Item | Measurement / budget |
| --- | --- |
| Backend idle resident memory | 15.2 MiB RSS |
| Authenticated telemetry + short history-request bursts | 16.1–17.5 MiB RSS; observed high-water mark 17.5 MiB |
| Practical runtime RAM allocation | Start with 128 MiB available to the backend; 256 MiB is a conservative budget for concurrency/large history queries; not load-validated |
| Release executable | Approximately 7.6 MiB on this x86-64 build |
| Built frontend with optional local wallpapers | Approximately 17 MiB; most bytes are the two wallpaper JPEGs |
| 90-day sample count | 518,400 rows at one sample every 15 seconds |
| Simulated 90-day sample database | 33.3 MiB using representative non-null values; actual SQLite encoding and values vary |
| Suggested runtime disk reservation | At least 256 MiB for binary, assets, database, transient WAL and growth; reserve 512 MiB–1 GiB if keeping backups/logs |
| Local build footprint observed | Rust `target/` 1.4 GiB, frontend `node_modules/` 88 MiB; reserve several GiB free to build |

The live-memory measurement used a small existing history database, not a full 90-day dataset. The 90-day disk simulation used the real sample schema, 518,400 timestamps and representative values. Browser/GPU memory, OS memory, compiler peak RAM, backups, and service logs are **not** included. Build peak RAM was not measured; building on a separate machine can keep production dependencies small. The 1.4 GiB build directory includes local build artifacts and is not a production requirement.

## Persistence and data interpretation

- SQLite WAL database: `data/monitor.sqlite3`. Samples are collected every 15 seconds while the backend runs with an active router session, even without an open browser.
- Metrics older than 90 days are deleted on startup and insertion. Freed pages are reused; the file need not shrink.
- Device labels live in `device_labels`, keyed by MAC, and are **not** subject to 90-day retention. Empty labels remove an entry.
- History contains metrics and online counts, not full client inventory. The label table contains MAC addresses and user-provided names: treat the database as private.
- No historical backfill. Gaps and counter resets mean traffic figures must not be treated as billing-grade usage.
- Throughput is derived from successive GPON counters, not a speed test. Total counters are not monthly usage.
- Wi-Fi band is matched against association tables where available, not inferred from ambiguous LAN port labels.
- Restart loses the router session, but not the database. Passwords are not persisted by the application.

Back up using SQLite's backup API/CLI while running, or stop the process before copying the database. Copying just the main database file during WAL writes can omit committed data. Never commit `data/`, cookie jars, or credentials.

## Security

Only point `ROUTER_URL` at a router you trust and administer. Self-signed TLS is accepted by the router HTTP clients (not a system-wide TLS setting). HTTPS certificate authenticity is therefore not verified; use a trusted LAN. Router redirects are currently followed with a limited redirect count, so this is not a strict network-level destination sandbox.

All router readings and labels are potentially sensitive. The UI renders strings through React; do not publish unredacted screenshots. Local browser labels are removed only after a successful database migration. Router cookies remain in process memory. Disconnect discards this application's session without a router-wide logout.

## Visual assets and license

Project-authored code is licensed under [MIT](LICENSE). Dependencies retain their own licenses, including `@zakisheriff/liquid-glass` (MIT).

Apple macOS Tahoe wallpaper JPEGs are included as third-party artwork at the project owner's request; no ownership or redistribution license is claimed. Source attribution is in [SOURCE.md](frontend/public/wallpapers/SOURCE.md). Attribution does not itself grant redistribution permission, and the project's MIT license does not grant rights to third-party artwork. A gradient fallback is available if the JPEGs are removed.

## Checks and contributions

```bash
cargo test
cargo clippy --all-targets -- -D warnings
npm run build --prefix frontend
```

Contributions for additional router adapters and sanitized parser fixtures are welcome. Keep router writes out of the monitoring API. Do not submit real credentials, cookies, labels/databases, serial numbers, or MAC inventories in issues or fixtures.
