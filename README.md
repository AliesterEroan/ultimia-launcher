# Ultimia Launcher

<div align="center">
  <img src="src/assets/ultimia-icon.png" alt="Ultimia Launcher Icon" width="128">
</div>

**Desktop launcher for the SoulWorker GB/Steam client — detects installs, launches official or private-server sessions.**

> Part of the **[Ultimia](https://github.com/AliesterEroan) suite**: [ultimiadash](https://github.com/AliesterEroan/ultimiadash) · [ultimia-server](https://github.com/AliesterEroan/ultimia-server) · [ultimia-sniff](https://github.com/AliesterEroan/ultimia-sniff) — four apps for running SoulWorker locally.

---

## 📜 Table of Contents

- [✨ Features](#-features)
- [🚀 Installation](#-installation)
- [📖 Usage](#-usage)
- [⚙️ Settings](#-settings)
- [🏗️ Architecture](#-architecture)
- [🛠️ Technologies Used](#-technologies-used)
- [⚠️ Limitations](#-limitations)
- [🔒 Privacy](#-privacy)
- [📜 License](#-license)
- [📧 Contact](#-contact)

---

## ✨ Features

### Install detection & launch
- **Multi-install detection** — finds both Steam (via Steam library manifests) and VFUN (via registry/path) installs and lists all of them; auto-detect keeps your chosen source.
- **Steam direct launch** — spawns `SoulWorker.exe` with the same `--ip GBL/CHN … --port` arguments Steam itself passes (verified byte-identical against a live Steam launch).
- **VFUN auth handoff** — refreshes saved tokens, mints a fresh auth code, and builds the official `VALOFE` launch string for the VFUN client.
- **Private-server launch** — points the client at the local Ultimia server emulator and auto-starts `ultimia-server.exe` when it is down (waits for the login port before launching).
- **Custom-server join** — PLAY PRIVATE opens a dedicated page: enter any `IP` or `IP:PORT`, an automatic TCP reachability check reports latency and the listening port, then PLAY hands the address to the client.

### Interface
- Dark orange "Ultimia" theme, animated startup sequence, server-select screen, per-launch process log viewer.
- **Emulator & dashboard controls** — the custom page's Emulator tab starts/checks the local server; the Dashboard tab's OPEN DASHBOARD button launches the standalone Ultimiadash app (or focuses its already-open window — never a duplicate).
- **Tray mode** — the launcher hides while the game runs and returns when the game exits; tray menu: Show launcher / Quit.
- **Appearance picker** — Settings → Advanced → Appearance shows two preview cards: **Midnight** (cool slate on near-black) and **Ember** (warm amber on toasted brown, matching Ultimiadash). Each card paints its own palette, so you see the look before choosing; a click applies it instantly. Colour-only — layout and controls are unchanged; the choice persists.
- Settings sections: Installation, Advanced, Profile, About.
- Self-contained window icon and in-app branding.

---

## 🚀 Installation

### Prerequisites
- Windows 10/11
- Node.js + npm (frontend build)
- Rust toolchain (Tauri backend build)
- A SoulWorker install (Steam `Soulworker_GB` or VFUN)

### Steps
1. `npm install` then `npm run build` in `launcher/` → produces `dist/`
2. `cd src-tauri && cargo build --release --features tauri/custom-protocol` → `target/release/ultimia-launcher.exe`
   - The `tauri/custom-protocol` feature is **required** — without it the exe tries to load the Vite dev server (`localhost:8443`) and shows `ERR_CONNECTION_REFUSED`.
3. Run the exe. It is fully self-contained — no dev server, no localhost dependency.

---

## 📖 Usage

### Launch on official servers
1. Settings → Installation → pick **Steam** or **VFUN**.
2. Back on the main screen choose **GLOBAL SERVER** → START.

### Launch on the private server
1. Settings → Advanced → point the emulator directory at the server checkout (the Ultimia workspace layout is also auto-detected).
2. Choose the local emulator option → START. The launcher starts the emulator itself if it is not running and waits for its login port.

### Join a custom server
1. PLAY PRIVATE → **Join Server** tab.
2. Type the server's `IP` or `IP:PORT` — the reachability check runs automatically and shows online/offline, latency and port.
3. **PLAY** launches the client pointed at that address.

### Open the server dashboard
- PLAY PRIVATE → **Dashboard** tab → **OPEN DASHBOARD**. The standalone Ultimiadash window opens (or is focused if already open, including from a minimized state).

### Reading launch results
- Success opens the game client; failure opens a modal with the captured process log — use **PROCESS LOG** for the raw outcome.

---

## ⚙️ Settings

- **Installation** — detected installs list, manual browse, selected source (Steam/VFUN).
- **Advanced** — emulator directory override, launch options, and the Appearance picker (Midnight / Ember preview cards).
- **Profile** — saved VFUN sign-in (tokens stored encrypted for the current OS user).
- **About** — launcher/game version info, credits.

---

## 🏗️ Architecture

```
launcher/
├── src/App.tsx              # React UI — screens, settings, launch flow
├── src/types/launcher.ts    # shared TS types (Tauri command mirrors, ThemeId, settings)
├── src/data/                # characters, constants, launch-error copy
├── src/utils/settings.ts    # load / persist / validate settings (localStorage)
├── src/index.css            # theme tokens ([data-theme] launcher / dashboard)
├── src-tauri/src/lib.rs     # Tauri commands registry
├── src-tauri/src/launcher/  # detect.rs / launch.rs / server.rs (install detection + launch)
├── src-tauri/src/vfun.rs    # VFUN auth: token refresh, auth code, VALOFE handoff
└── src-tauri/tauri.conf.json# window + bundle config (custom-protocol for prod)
```

### Data Flow
1. Frontend calls `detect_install` → Rust scans registry + Steam manifests → returns all installs.
2. `launch` routes: Steam source → direct `SoulWorker.exe` + official `--ip` args; VFUN → auth handoff; private → `emulator_ensure` (spawn + port watch if down) → emulator hand-off; custom → validated address hand-off.
3. Outcome (stdout/stderr, exit) streams back to the UI log viewer; `watch_game` hides the window until the game process exits.

---

## 🛠️ Technologies Used

### Core
- **Rust** — backend: detection, process spawning, auth, token storage
- **React 19 + TypeScript** — frontend UI
- **Vite 8 + Tailwind CSS v4** — build + styling

### Desktop
- **Tauri 2** — webview shell; `custom-protocol` feature embeds `dist/` for self-contained exe
- **Lucide** — icons

---

## ⚠️ Limitations

### Technical
- Steam launch uses the official server IPs baked into Steam's launch args — it does not fix official-server availability problems (those are server/network-side).
- VFUN auth requires a saved sign-in; accounts needing extra verification must use the official launcher once.
- Detection covers Steam and VFUN layouts only.

### Testing Limitations
- Launch paths verified against live Steam/VFUN processes on the dev machine (command lines compared byte-for-byte).
- What a clone cannot re-check: official-server connectivity and VFUN auth depend on live external services and a real account.

---

## 🔒 Privacy

- Sign-in tokens are stored encrypted for the current OS user only; passwords are never persisted.
- No telemetry; the launcher only talks to the game client, Steam, VFUN auth endpoints, and the local emulator when selected.

---

## 🖼️ Asset Credits

Character artwork under `src/assets/characters/` is official SoulWorker fan-kit art (© Gamecom / official SoulWorker fan kit terms), used under the kit's non-commercial fan-use license. The `ultimia-icon.png` and fonts (Chakra Petch, Rajdhani, Teko, Inter — SIL Open Font License) are project assets.

---

## 📜 License

Ultimia Launcher is proprietary software — see [LICENSE](LICENSE) for the full terms.

**Copyright © 2026 AliesterEroan. All rights reserved.**

Personal, non-commercial use only. SoulWorker and all game assets belong to their respective owners; this project contains no game files.

## 📧 Contact

- GitHub: [AliesterEroan/ultimia-launcher](https://github.com/AliesterEroan/ultimia-launcher)
- Issues: [Create an issue](https://github.com/AliesterEroan/ultimia-launcher/issues)

---

Built with ❤️ by AliesterEroan
