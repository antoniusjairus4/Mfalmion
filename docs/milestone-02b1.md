# Milestone 02B.1 — Smooth Piece Animations and Drag-and-Drop Refinement

Implemented 2026-11-16. Milestone 02B is accepted per this request; native visual acceptance of this refinement is pending. No native GUI was launched, and no OS graphical automation, display-manager/virtual-terminal/driver command, system installation, engine or AI server was used. No commits or pushes. Milestone 02C is not started.

## Architecture

- `board/animation.rs` (new): reusable presentation-only `PieceAnimation` and `AnimationState`. Each track stores the rendered piece, source, destination, start time, duration and canonical origin. `transition(before, after, now)` accepts immutable consecutive valid position snapshots, independently of mouse input, so future replay can request transitions without introducing PGN or navigation controls now.
- `board/state.rs`: reversible orientation projection for fractional board-cell coordinates. Canonical a1 center is [0.5, 0.5]; coordinates are never stored in pixels.
- `board/view.rs`: one shared geometry for input, square centers and motion. Rendering now explicitly orders squares, highlights, stationary pieces, animated/dragged pieces, then overlays. Animated destinations and drag sources are suppressed in the stationary pass.
- `ui/game_review.rs`: owns animation state, commits legal library moves immediately and then requests visual transitions. Pending promotion, draw claims and reset controls retain their existing rules behavior. Helpers implement deterministic Undo/New Game/Flip interruption handling.
- `app.rs` and README: milestone status and instructions.

`board/game.rs`, snapshots/role adapters, shakmaty 0.30.1, Cburnett artwork/attribution, resvg texture cache, green/cream palette, graphics policy and dependencies are unchanged. No independent chess rules or move generator was introduced.

## Timing and interpolation

Normal click moves use **200 ms** with ease-out cubic:

```text
progress = clamp((now - start) / duration, 0, 1)
eased = 1 - (1 - progress)^3
point = origin + (destination - origin) * eased
```

At half elapsed time the piece has traversed 87.5% of the distance; the curve is monotonic and does not overshoot. Zero/negative duration completes immediately. Absolute egui time determines progress, with no per-frame accumulated movement or blocking waits.

Canonical positions are projected through the current orientation and current board rectangle on every draw. Resizing/zooming therefore does not strand a piece at stale pixel coordinates. Flip deliberately cancels motion rather than changing direction mid-flight.

`advance(now)` discards completed tracks before rendering, revealing the authoritative destination piece once, without a duplicate. The renderer requests another repaint while tracks remain active. When the board is static, this layer requests no continuous repaint. Existing one-shot input/controls/egui widget repaint behavior remains intact.

## Drag behavior

Only pieces belonging to the side to move receive drag interactions. The cached SVG is painted centered at the pointer above stationary pieces; the source image is suppressed. Pointer events provide the updates without a timer or lagging interpolation filter.

On release, a legal move commits immediately. The primary piece settles for **80 ms** from the actual fractional release location to the destination center. A centered drop creates no redundant primary animation. In drag castling the rook still starts its 200 ms transition at the same timestamp. The renderer retains the pointer image on the release frame, avoiding a one-frame jump back to the source before settling.

Illegal or off-board drops do not change the chess state. A **120 ms** snap-back animates the piece from its release location to its original square. Off-board points remain valid presentation coordinates; only the hit test may return no destination. The page clip may hide a pointer image outside the visible content area, but the source and rules state remain intact.

## Special moves

- Castling: position-difference matching identifies the king and rook arrivals and starts both tracks together at their correct squares. No duplicate destination king/rook is drawn.
- Captures: captured pieces disappear from the committed authoritative position; they do not receive tracks or remain as ghosts.
- En passant: the committed position removes the captured pawn from its actual capture square while the capturing pawn glides to the empty destination.
- Promotion: no move/animation is committed while the chooser is open. After Queen/Rook/Bishop/Knight confirmation, the chosen piece animates from the pawn's source to the promotion square; the authoritative destination already contains it. Cancel leaves the position unchanged.
- Undo: restores the existing full shakmaty snapshot and cancels visual motion immediately. Undo itself is intentionally not animated; prior castling rights, EP/counters and repetition history remain handled by the unchanged session implementation.
- Draw claims do not execute declared moves and therefore create no fictitious move animation.

## Interruption strategy

- New Game: resets the game and clears tracks, selection, promotion and pending claim input; orientation is retained as before.
- Undo: cancels tracks and pending interaction, then shows the restored authoritative position.
- Flip: cancels tracks, flips orientation, retains logical game state/selection.
- Control buttons also stop egui's active drag tracking and request a one-shot repaint.
- A new legal move replaces the old visual transition, starting from the now-authoritative previous position. Unfinished earlier movers immediately appear at their already committed destinations. There is no queue of stale animations.
- Beginning a new drag finishes earlier visual motion. Navigating away does not run a background loop; when the board returns, elapsed time discards old tracks.
- Closing requires no animation completion, thread join or wait.

The snapshot interface targets consecutive standard-chess transitions. It pairs changed arrivals with matching removed pieces, with a promotion-role adaptation. Arbitrary multi-ply jumps are not a replay feature; unmatched arrivals simply appear at the final authoritative position. PGN parsing, a move-history UI and Next/Previous controls remain excluded.

## Performance and preservation

No dependency or animation framework was added. Interpolation and geometry require no per-frame motion allocations. Track vector capacity is reused between transitions; temporary position-difference matching uses a fixed stack array. The existing cached SVG textures are used for stationary, moving and dragged pieces at the same cell size, so motion alone does not trigger rasterization. Raster-size changes retain the existing cache policy.

These are implementation properties, not measured FPS/CPU claims. Native smoothness and physical pointer behavior require manual acceptance. The existing 02B conservative dead-position limitation and original Wayland surface incompatibility remain unchanged.

## Automated verification

Passed:

```sh
cargo fmt --all -- --check
cargo check --workspace --offline
cargo test --workspace --offline
cargo clippy --workspace --all-targets --offline -- -D warnings
git diff --check
```

**41 tests passed; zero failed.** Existing rules, asset/cache and board tests remain passing. Twelve added tests cover start/midpoint/completion easing and zero-duration handling, frame-rate-independent absolute sampling, source/destination and flipped coordinates, resize reprojection, simultaneous castling, capture/en passant rendering state, all promotion piece types, no position mutation, drag settling/snap-back, delayed promotion confirmation, rapid legal moves, and Undo/New Game/Flip during animation.

The tests are CPU-only unit/egui-pass tests; they create no desktop window and use no graphical-session automation. An initial test compilation error from naming a private module was corrected before the final passing checks.

## Manual acceptance

Run from the repository in your graphical terminal using the accepted workaround:

```sh
env -u WAYLAND_DISPLAY MFALMION_WINDOW_SYSTEM=x11 VK_DRIVER_FILES=/usr/share/vulkan/icd.d/intel_icd.json WGPU_BACKEND=vulkan cargo run -p mfalmion-app --offline
```

- [ ] Click e2 then e4. Confirm a roughly 200 ms glide, no duplicate pawn at e4, and Black to move immediately. Continue e7-e5 rapidly; ensure both committed positions are correct.
- [ ] Drag a legal piece and hold still: it should remain centered below the pointer with no source duplicate. Drop near an edge of a legal destination; it should settle a short distance, without replaying the full move.
- [ ] Drop illegally and outside the board. Confirm snap-back, unchanged turn/position, and no stranded piece.
- [ ] Test white castling with e2-e4, e7-e5, g1-f3, b8-c6, f1-c4, g8-f6, e1-g1. Both king and rook should move together. Repeat with drag castling.
- [ ] Test a normal capture and en passant using e2-e4, a7-a6, e4-e5, d7-d5, e5-d6. Confirm the captured pawn disappears from d5, without a ghost.
- [ ] Promote during play; verify the chooser holds the position unchanged, all four choices are correct, and only confirmation starts the promoted-piece animation.
- [ ] Trigger Undo, New Game and Flip quickly while a click move is animating. Check convergence to the correct authoritative position every time. Undo should restore state instantly rather than start a reverse animation.
- [ ] Resize and zoom during motion; repeat with a flipped board. Confirm square mapping and crisp artwork.
- [ ] Leave the board idle, navigate away/back, and close during motion. No animation should need to finish before the app closes.

## Storage and Git

Free internal SSD space before: approximately **7.8 GB**. After checks: approximately **7.8 GB** (rounded df -h). Project target/: approximately **2.1 GB** (rounded du). Existing build artifacts were reused; no release build, downloads, system packages, cache deletion, cargo clean, external-archive or AI asset modifications.

Git changes: README; app.rs; board/mod.rs, state.rs and view.rs; ui/game_review.rs; new board/animation.rs and this document. Changes remain local and uncommitted. Stop at 02B.1 for user acceptance; do not begin 02C.
