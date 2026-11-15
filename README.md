# Mfalmion

A local-first chess analysis desktop application for Kali Linux Rolling, built with Rust, egui/eframe, and wgpu. It opens a dedicated Linux desktop window.

## Current milestone: 01 — Rust Desktop Foundation

The shell provides six navigable placeholder pages, a responsive dark theme, and application status. Stockfish, AI coaching, Chess.com sync, databases, and chess rules are **not implemented**. No statistics or analysis are fabricated.

## Development on Kali Linux

Use the installed compatible Rust toolchain (verified with Rust 1.99; workspace minimum 1.95), Cargo, a C linker, and existing X11/Wayland and Vulkan development/runtime libraries. Run from a graphical desktop session. No JavaScript tooling or system package installation is needed by this repository.

```sh
cargo run -p mfalmion-app
```

Verification:

```sh
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

wgpu is selected explicitly; there is no GL/framework fallback. If startup fails, inspect terminal diagnostics and `vulkaninfo --summary`, and check graphical-session access. The UI follows the desktop scale factor; egui's standard Ctrl/Cmd-plus/minus zoom is available.

Builds use two jobs, no debug symbols or incremental cache, and store artifacts in `target/` to conserve disk and memory. Check `df -h .` before and after building and `du -sh target` for artifact size. Do not clean caches or install packages automatically. Cargo.lock belongs in version control. Where dependencies are already cached, add `--offline` to avoid network access.

## Layout

- `crates/mfalmion-app/src/main.rs`: native startup, explicit wgpu renderer, terminal lifecycle/error logs.
- `app.rs`: shell composition and current page state.
- `theme.rs`: reusable palette and style.
- `ui/`: navigation and truthful placeholder screens.
- `AGENTS.md`: permanent engineering rules.
- `docs/`: milestone verification notes.
- `assets/`: reserved for future approved local assets; no assets are required now.

## Planned milestones

02: Interactive Chessboard, subject to approval. Later approved milestones will add Stockfish UCI analysis/play, PGN and a SQLite library, reviews and statistics, optional Chess.com synchronization, and local AI explanations via llama.cpp. Stockfish evaluations will remain separate from AI explanations. Detailed sequencing beyond Milestone 02 is not yet committed.

No external AI assets or CUDA installations are changed. The shell starts no engine, AI server, or network integration. Development proceeds one milestone per explicit approval; no automatic commits or pushes.

## Linux graphics compatibility

The shell defaults to **X11 when DISPLAY is available**, and **wgpu Vulkan with a low-power adapter preference**. On this GNOME Wayland session, X11 means XWayland; no desktop-session settings are changed. Intel Iris Xe is selected on this laptop. This avoids the NVIDIA 550.163.01 Wayland surface failure observed with eframe's high-performance default, and leaves the shell off the discrete GPU by default.

Run normally; no ICD environment variables are required:

```sh
cargo run -p mfalmion-app --offline
```

For explicit session diagnostics:

```sh
MFALMION_WINDOW_SYSTEM=wayland cargo run -p mfalmion-app --offline
MFALMION_WINDOW_SYSTEM=auto cargo run -p mfalmion-app --offline
MFALMION_DIAGNOSTICS=1 cargo run -p mfalmion-app --offline
```

`MFALMION_WINDOW_SYSTEM` accepts `x11`, `wayland`, or `auto` (winit's native selection). Without DISPLAY, native selection is retained. Existing `WGPU_BACKEND` and `WGPU_POWER_PREF` overrides are respected; selecting `WGPU_POWER_PREF=high` can reproduce the failing NVIDIA path. There is no automatic renderer retry or framework switch. Startup reports the session policy and actual GPU; navigation and clean shutdown are logged to stderr. Debug graphics logs are opt-in.

Runtime verification passed startup, Vulkan rendering, keyboard navigation across all six pages, resizing to 760×540 and back to 1440×900, and a normal WM_DELETE_WINDOW close with exit status 0. Automated pointer injection under GNOME Wayland was unreliable; physical mouse and title-bar-close checks remain manual. Native Wayland with low-power Intel Vulkan started without the former surface panic, but full interaction on that path was not verified. A native Xorg session was not tested.

See [runtime stabilization findings](docs/runtime-stabilization.md) for diagnostics and a manual acceptance checklist.
