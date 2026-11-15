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

## Milestone 01 runtime caveat

In the development session, the default Wayland launch failed inside wgpu with `Surface::configure: Invalid surface`. X11 produced visible UI frames, including the compact layout, but automated navigation and normal shutdown could not be reliably verified. A graphics/session compatibility issue remains unresolved; the rendered frame is not proof of sustained responsiveness.

To reproduce the Intel Vulkan diagnostic run on this machine:

```sh
env -u WAYLAND_DISPLAY VK_DRIVER_FILES=/usr/share/vulkan/icd.d/intel_icd.json WGPU_BACKEND=vulkan cargo run -p mfalmion-app
```

This is an explicit diagnostic selection of X11/Intel Vulkan, not an automatic fallback. Manually test all six pages, resize to 760×540, scroll Settings, and close the window. Do not proceed to Milestone 02 until runtime behavior is accepted.
