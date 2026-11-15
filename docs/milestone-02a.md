# Milestone 02A — Native Interactive Chessboard UI

Implemented 2026-11-16. Manual graphical acceptance is pending. No GUI application was launched and no graphical automation was performed for this checkpoint. Only CPU/headless tests were run.

## Accepted Milestone 01 runtime

The user has manually verified native-window startup, navigation, resizing, and clean shutdown using X11/Intel Vulkan. That is Milestone 01's accepted runtime configuration. Under the current GNOME Wayland session the X11 client uses XWayland. The earlier Wayland Invalid surface issue remains unresolved; graphics policy, driver settings, and dependencies were preserved for 02A.

## Implementation

Game Review contains a native egui chessboard with 64 alternating square cells, rank/file labels, and the standard 32-piece position. White starts at the bottom; a1 is dark and h1 light. Queens are on d1/d8 and kings on e1/e8.

Starting position represented by the snapshot:

```text
rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1
```

This string is documentation, not a FEN parser. The display snapshot intentionally contains no castling, en-passant, clocks, turn, or legality state.

Click selects a square; another square replaces it; clicking the selected square clears selection. The inspector displays the coordinate. Flip Board rotates the display 180 degrees while keeping the logical selection and position intact. Hover/focus uses purple and selection uses a cyan overlay/border, so clearing selection removes the cyan indication. UI state persists while navigating between pages for the lifetime of the process; nothing is saved to disk.

The inspector includes truthful future move-history and engine-evaluation placeholders. Wide layouts put it beside the board; smaller layouts stack it below, within the existing page scroll area. Board dimensions stay square and are bounded by the available width and viewport height. Existing sidebar, theme, other pages and X11 compatibility configuration are retained. The shared status now identifies Milestone 02A.

## Architecture and coordinates

- `board/square.rs`: validated zero-based files/ranks, algebraic labels, and square color. Index is rank × 8 + file, a1=0, h8=63; no invalid square can be created through its constructor.
- `board/state.rs`: logical selection and orientation. Display row 0 is the top and column 0 the left. White-bottom maps (column,row) to (column,7-row); black-bottom maps to (7-column,row).
- `board/position.rs`: six piece kinds, two sides and an immutable read-only 64-slot starting snapshot. It implements no move rules. A later chess-library adapter can populate an equivalent display snapshot while retaining the renderer.
- `board/view.rs`: egui rectangle/text painting, coordinate labels, piece font registration and per-square input hit targets. Rendering and hit testing use the same mapped rectangles. Input delegates selection changes to BoardState; the renderer cannot mutate Position.
- `ui/game_review.rs`: responsive board/inspector composition and Flip Board input.
- `app.rs`: owns Game Review state and routes that page. `theme.rs` supplies all new square/piece colors.

The pieces use six filled Unicode silhouettes from a bundled 20 KB subset of the installed DejaVu Sans font (U+265A–U+265F). White/black fill and contrasting outlines distinguish sides on both square colors. It is a dedicated font family and does not replace the application's text fonts. `assets/fonts/LICENSE-DejaVu.txt` preserves provenance, copyright, trademark and permission terms; the subset's font names were changed to Mfalmion Chess. The asset is embedded with include_bytes!, so no runtime network access or system font installation is required.

Asset generation used the already installed pyftsubset/fontTools utilities once during development; they are not build or runtime dependencies. No Cargo dependencies, lockfile changes, or asset downloads were needed.

## Automated verification

Passed:

```sh
cargo fmt --all -- --check
cargo check --workspace --offline
cargo test --workspace --offline
cargo clippy --workspace --all-targets --offline -- -D warnings
git diff --check
```

Eight tests passed (seven board tests and the existing graphics-policy test). Coverage includes exactly 64 valid coordinates, all starting pieces and empty squares, a1/h1 colors and alternation, every display-coordinate mapping in both orientations, queens/kings, flip/selection behavior, and actual headless egui pointer press/release hit testing at 280- and 640-point board sizes. The headless test counts all 64 painted cells and verifies each cell's square geometry at both sizes. FontTools additionally verified that every bundled chess code point exists.

These tests do not establish native visual quality, GPU presentation, OS pointer input, or shutdown behavior for this checkpoint. Those checks are reserved for the user's manual test.

## Manual launch and acceptance

From the repository, use the known-working explicit X11/Intel Vulkan configuration:

```sh
env -u WAYLAND_DISPLAY MFALMION_WINDOW_SYSTEM=x11 VK_DRIVER_FILES=/usr/share/vulkan/icd.d/intel_icd.json WGPU_BACKEND=vulkan cargo run -p mfalmion-app --offline
```

The existing per-application compatibility defaults also remain available with `cargo run -p mfalmion-app --offline`.

- [ ] Open Game Review; verify an 8×8 board, readable silhouettes and coordinates, with white at the bottom.
- [ ] Verify a1 dark, h1 light; white queen d1/king e1 and black queen d8/king e8. Check all 32 pieces and the four empty middle ranks.
- [ ] Click a1, then e4: selection and displayed coordinate should follow. Click e4 again: cyan selection clears and inspector says None. No piece moves.
- [ ] Select d1 and flip: black is now at the bottom, bottom-left is h8, top-left is h1, and d1 remains selected at its rotated location. Flip again to restore the original view.
- [ ] In the flipped view click both an occupied and an empty square and check their coordinates. Test selection on light and dark squares.
- [ ] Resize between approximately 1440×900 and 760×540. The board must stay square. The inspector should move below the board on narrow windows; scroll to reach it and test Flip Board. No content should overlap.
- [ ] Visit the other five pages and return; state should persist and other pages should retain their layouts.
- [ ] Close normally; confirm shutdown and restart normally. No engine, model server or network integration should start.

## Resources and limitations

Free SSD space before checks: approximately 8.5 GB; after: approximately 8.5 GB (df -h, rounded). Project target/: approximately 1.4 GB. Bundled font/license directory: 28 KB on disk. Existing Cargo artifacts were reused; no release build, cache removal, package installation, external-drive/archive access, or AI asset/server changes occurred.

Native visual acceptance remains pending. The board only shows the starting position and local selection/orientation state. No moving/dragging pieces, legal move logic, PGN/FEN loading, turn management, move history, evaluation, engine, database, or network integration exists. These absences are intentional.

## Next checkpoint and Git

Milestone 02B requires explicit approval and will introduce a maintained Rust chess library for legal moves, replacing/adapting the display snapshot rather than building a second chess-rules implementation. Neither 02B nor 02C was started.

This request explicitly overrides the earlier standing push preference: no commit or push was performed. Existing Git history and remotes are preserved.
