# Mfalmion

A local-first chess analysis desktop application for Kali Linux Rolling, built with Rust, egui/eframe, and wgpu. It opens a dedicated Linux desktop window.

## Current milestone: 02C — PGN/FEN and Game Replay

Game Review contains a native chessboard with legal click-to-move and drag-and-drop, legal-destination markers, turn/check/mate/stalemate status, castling, en passant, promotion choices, board flipping, and New Game, and full-state Undo Last Move. Click moves now glide for 200 ms; legal drags settle briefly and invalid drops snap back. Both sides are controlled locally; White moves first. Threefold repetition and the 50-move rule are claimable; fivefold repetition and 75 moves are automatic draws. Checked kings are highlighted. The other five sections retain their placeholders.

Rules and move validation use [shakmaty 0.30.1](https://docs.rs/shakmaty/0.30.1/shakmaty/) (GPL-3.0-or-later). Stockfish, AI coaching, Chess.com sync, databases, and review calculations are **not implemented**. No statistics or engine evaluations are fabricated.

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
- `board/`: authoritative shakmaty game/session state, renderer snapshot adapter, coordinate/orientation mapping, input events, and native egui painting.
- `ui/`: navigation, Game Review layout, and truthful placeholder screens.
- `AGENTS.md`: permanent engineering rules.
- `docs/`: milestone verification notes.
- `assets/pieces/cburnett/`: twelve offline SVG pieces by Colin M. L. Burnett, from the pinned Lichess source under GPLv2-or-later. Attribution, source SVGs and full license are bundled. The old font subset remains a historical unused asset and is no longer embedded or loaded.

## Planned milestones

02A: Native board UI. 02B: Legal moves implemented. 02B.1: Presentation-only move animations and drag refinement implemented; manual acceptance pending. 02C adds PGN import/export, FEN sessions, indexed replay, move history, explicit local variations and an on-board graphical promotion chooser; manual acceptance is pending. Later approved milestones will add Stockfish UCI analysis/play, PGN and a SQLite library, reviews and statistics, optional Chess.com synchronization, and local AI explanations via llama.cpp. Stockfish evaluations will remain separate from AI explanations. Detailed sequencing beyond Milestone 02 is not yet committed.

No external AI assets or CUDA installations are changed. The shell starts no engine, AI server, or network integration. Development proceeds one milestone per explicit approval. Milestone 02B and its visual refinement were committed/pushed under the subsequent user authorization. The 02C checkpoint was subsequently committed and pushed under explicit user authorization. The final audit fixes remain local and uncommitted for acceptance.

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

The user has now manually accepted Milestone 01 startup, navigation, resizing, and shutdown using **X11/Intel Vulkan**. The original Wayland Invalid surface issue remains unresolved; the compatibility configuration is preserved.

For Milestone 02B manual testing with the known-working explicit workaround:

```sh
env -u WAYLAND_DISPLAY MFALMION_WINDOW_SYSTEM=x11 VK_DRIVER_FILES=/usr/share/vulkan/icd.d/intel_icd.json WGPU_BACKEND=vulkan cargo run -p mfalmion-app --offline
```

To move: select a piece of the side to move, then click a mint-marked destination, or drag the piece there. Illegal drops snap back. Castle by moving the king to its usual c/g destination; promotion waits for your piece choice. New Game starts again; Undo Last Move restores rights, en passant, counters, and repetition history. Use the scrollbar to reach controls on smaller windows.

No GUI was launched or graphically automated during 02A/02B. See [Milestone 02B](docs/milestone-02b.md) for movement verification and limitations, [Milestone 02A](docs/milestone-02a.md) for the original board checkpoint, and [runtime stabilization findings](docs/runtime-stabilization.md) for the historical diagnostics.

## Chessboard artwork

The board uses cream `#EEEED2` and green `#769656` squares with classic Cburnett SVG pieces. Artwork is rasterized using `resvg` (default features disabled) at display-density-aware size buckets. Parsed trees and textures are cached, with at most one live texture size for each of 12 pieces. There is no runtime download or system chess-font dependency.

Artwork: Colin M. L. Burnett, GPLv2-or-later, selected from Lichess’s asset-specific grant. See [attribution and redistribution notices](assets/pieces/cburnett/ATTRIBUTION.md) and [visual refinement verification](docs/chessboard-visual-refinement.md). Rules, session state, controls, application theme and X11 setup are unchanged. Native visual acceptance is pending; no GUI was launched for this refinement.

## Movement animations

Click-to-move uses elapsed-time ease-out cubic motion (200 ms). Legal drops settle from the actual pointer release point (80 ms; no replay from the source), and invalid/off-board drops snap back (120 ms). The chess position updates before the visual transition. Castling moves both pieces; captures and en passant remove captured pieces immediately; promotion waits for confirmation. Undo and Flip Board finish/cancel active visual motion, while New Game clears motion and pending interactions. Resizing reprojects board-cell coordinates into the current geometry. No animation framework or dependency was added.

See [Milestone 02B.1](docs/milestone-02b1.md) for timing, interruption behavior, test coverage, and the manual acceptance checklist. No GUI was launched for this checkpoint.

## PGN/FEN and replay

Game Review can import multiple games from a selected PGN file or pasted text, show metadata and SAN history, navigate with first/previous/next/last controls, and copy/export PGN. Left/Right and Home/End navigate when text input is not focused. FEN position controls validate all six fields before replacing the session and display/copy the current FEN.

Imported games are read-only until **Create local variation from here** is selected. Historical play also requires that action; the forward continuation is preserved. **Return to original mainline** restores the branch source. Undo moves backward without deleting the continuation. Export/copy uses the active mainline or explicitly created local variation. New Game starts a fresh session.

Promotion now uses four cached graphical choices on the board. Click Queen/Rook/Bishop/Knight; Escape or clicking outside cancels. The selector works in both orientations. Castling remains an ordinary move.

File dialogs use the existing Linux `zenity` executable, only on explicit button clicks; dialogs, file IO, and PGN parsing run in a worker. No package installation is required. Paste/copy remains available if native dialogs are unavailable. Imports are limited to 16 MiB. No external archive is scanned or loaded. See [Milestone 02C](docs/milestone-02c.md) for architecture, tests, limitations and manual acceptance.

The [Milestone 02C final audit](docs/milestone-02c-audit.md) fixes result/adjudication handling, guarded session replacement, metadata spacing and file-worker reliability. The audit suite has 72 passing tests; manual native acceptance remains required. Imports retain the 16 MiB limit and additionally allow at most 200,000 total snapshots and 4,096 plies per game. PGN text must include a termination marker. Local work triggers an explicit replacement decision before switching sessions.
