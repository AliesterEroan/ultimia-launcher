# Ultimia Launcher

Tauri 2 desktop app: React 19 + Vite 8 + Tailwind v4 frontend (`src/`), Rust backend (`src-tauri/`). Player-facing launcher for SoulWorker (official VFUN + private servers). Not a web app — the production exe embeds `dist/` and runs offline.

## Commands

- Frontend build (type-checks first): `npm run build` → `dist/`
- Type-check only: `npx tsc --noEmit`
- Backend tests: `cargo test --manifest-path src-tauri/Cargo.toml`
- Backend lint: `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`
- Release exe: `cd src-tauri && cargo build --release --features tauri/custom-protocol`
- Dev (full app): `npm run tauri:dev` · frontend only: `npm run dev` (port 8443)

## Gotchas

- IMPORTANT: the release exe MUST be built with `--features tauri/custom-protocol`. Without it the exe loads the Vite dev server (`localhost:8443`) and shows a blank / connection-refused window.
- `base: './'` in `vite.config.ts` is required — the custom-protocol shell loads assets by relative path. Don't change it.
- The frontend talks to Rust only through Tauri `invoke()`. Commands are registered in `src-tauri/src/lib.rs`; logic lives in `src-tauri/src/launcher/` (`detect.rs`, `launch.rs`, `server.rs`) and `vfun.rs` (official auth). Serde structs are camelCase; their TS mirrors live in `src/types/launcher.ts` — keep both sides in sync.
- `src/App.tsx` is still a large single-file UI (types, data and settings helpers already extracted to `src/types/`, `src/data/`, `src/utils/`). Make surgical edits; propose a component split before restructuring.
- Errors cross the boundary as `Result<T, String>` with user-readable messages; the UI maps known prefixes (e.g. `vfun-auth-failed:`).

## Hard rules

- Zero game-file writes. Never create/edit/delete anything inside a detected SoulWorker install dir. Reading game files and launching the exe with args is fine.
- Never log, print, or commit auth material: tokens, auth codes, account keys, device ids, passwords. VFUN sign-in is DPAPI-encrypted under `%APPDATA%\UltimiaLauncher\`; passwords are never persisted.
- No hardcoded secrets or machine-specific paths in committed code — env vars, config, or relative paths only.
- The Steam client ignores `--ip` (keeps official IPs, honors `--port`); don't rely on args to redirect it.

## More

- Architecture, usage, settings: `README.md`
- Suite-wide conventions: the `ultimia-stack`, `ultimia-backend`, `ultimia-ui` skills.