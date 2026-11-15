# Milestone 02B — Legal Chess Moves and Rules Integration

Implemented 2026-11-16. Native graphical acceptance is pending. No desktop application, graphical automation, virtual-terminal command, display configuration tool, engine, or AI server was launched during this checkpoint.

## Prerequisite and scope

Read AGENTS.md, README, Milestone 01 and Milestone 02A documentation. The original 02A board checkpoint passed eight tests and was safely integrated; those board/mapping checks continue to pass. The earlier user request authorized pushing 02A: commit 37ab8be is already on GitHub. The later detailed 02B request prohibits commits/pushes, so **02B remains uncommitted**. No 02C, Stockfish, SQLite, Chess.com, Qwen, PGN interface, or complete move-history UI was added.

The accepted X11/Intel Vulkan runtime configuration, native egui/eframe/wgpu architecture, dark theme and sidebar are preserved. The original Wayland Invalid surface problem remains unresolved.

## Rules architecture

- `shakmaty = "=0.30.1"` is the authoritative rules dependency (GPL-3.0-or-later). It satisfies the workspace Rust 1.95 minimum; 0.30.2 requires 1.97. The installed compiler is 1.99. No variants are enabled.
- `board/game.rs`: standard-chess session state, legal moves, authoritative `shakmaty::Chess`, status, full previous-position snapshots, repetition, draw claims and undo. No egui dependency in this module. There is no independent move generator.
- `board/position.rs`: a fresh read-only display adapter from the authoritative board. It maps piece roles/colors into renderer types. The UI does not store or mutate a separate chess position.
- `board/view.rs`: native painting and mapped click/drag events. Mint dots mark quiet destinations; capture rings include en passant, even though its destination square is empty. The checked king has a red background/marker. Orientation mapping and 64-square geometry are retained.
- `ui/game_review.rs`: selection and movement controller, responsive controls, native promotion chooser, and declared draw-claim interaction. Selection/orientation are presentation state only.
- `theme.rs`: shared check-indicator color. `app.rs`: current milestone status.

Only the small shakmaty/btoi packages were newly needed for the final dependency graph; other dependencies were cached. The interim uncommitted cozy-chess choice from the earlier broad movement request was removed from Cargo.toml and Cargo.lock. Existing caches/artifacts were not deleted.

Reference: [shakmaty Position](https://docs.rs/shakmaty/0.30.1/shakmaty/trait.Position.html), [Chess equality](https://docs.rs/shakmaty/0.30.1/shakmaty/struct.Chess.html).

## Interaction and special moves

Select a piece of the side to move, then click a marked destination, or drag it there. Clicking the selected piece deselects; another friendly piece reselects. Illegal moves/drops leave the board and turn unchanged. Dropping outside the board returns the piece to its source. Successful moves clear selection and update the authoritative position immediately. Both sides are controlled locally; there is no engine opponent. Flipping changes only display orientation.

Castling uses normal king destinations c1/g1/c8/g8, adapted to shakmaty's king/rook castle representation. Both pieces move through library execution; lost rights, check, attacked transit/destination squares and blocked paths are rejected by the library.

En passant uses legal library moves, removes the correct pawn, expires correctly, and is rejected when it would expose the king. Promotion leaves the position unchanged while the native chooser awaits Queen/Rook/Bishop/Knight; cancel leaves the original position intact. Board input is disabled while choosing and after a terminal result.

New Game resets the session, history, draw claim, pending promotion and selection, retaining board orientation. Undo Last Move restores the entire preceding `Chess` snapshot, not a reverse move, including counters, castling and en passant. It truncates repetition history and clears claims/pending UI actions. If a draw was claimed, undo also removes that adjudication; if a move exists, the preceding move is undone. With no moves, a root-position claim can simply be cleared. Empty-history Undo is disabled unless a pending promotion/claim needs clearing.

## Draw rules and limitations

Based on [FIDE Laws, Articles 9.2–9.6](https://handbook.fide.com/chapter/E012023):

- Threefold repetition and 100 halfmoves without a pawn move or capture enable **Claim Draw**; they do not automatically stop normal play.
- **Claim draw with next move** lets the side to move declare a legal source/destination (and promotion choice if relevant) that would satisfy a threefold or 50-move claim. A successful claim ends the session without executing that declared move. An invalid claim leaves the board untouched; uncheck the option to resume ordinary movement.
- Five occurrences of the position or 150 halfmoves end the game automatically. Library checkmate is adjudicated first, so a mating move at halfmove 150 wins rather than drawing.
- Repetition uses exact `Chess` equality, not a collision-prone hash: piece placement, turn, castling rights and only legally capturable en passant state matter; clocks do not. Full snapshots from the known initial position preserve all session occurrences, and undo removes the discarded branch.
- Stalemate and insufficient material are delegated to shakmaty's outcome. There is no broad custom material heuristic; two knights versus a bare king is not declared drawn just because forced mate is unavailable.
- **Limitation:** arbitrary locked/dead positions beyond the library's conservative insufficient-material detector are not solved. No claim of complete dynamic dead-position adjudication is made. There are no clocks, resignation, draw agreement, competition arbiter/penalty rules, or persistence between app launches. These are outside this checkpoint.

## Verification

Passed:

```sh
cargo fmt --all -- --check
cargo check --workspace --offline
cargo test --workspace --offline
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo build -p mfalmion-app --offline
git diff --check
```

27 tests passed, zero failed. Rules coverage includes initial FEN/20 legal moves, both turns, invalid moves, captures, both castlings, castling out/through/into check, lost king/rook rights, en passant and expiration/exposed-king rejection, all four promotions and black promotion, check/checked king, mate, stalemate, full-state undo, current/intended draw claims, threefold versus fivefold, 50 versus 75 moves, mate precedence, repetition equivalence, and conservative material detection.

Existing renderer/controller unit tests run CPU-only egui passes without any native window, display connection or GPU. They cover square geometry, click selection, click-to-move, flipped drag-to-move, invalid/off-board drops and wrong-side input. They do not replace manual desktop acceptance. No automated OS GUI tests were performed.

## Manual X11/Intel test

From the repository in a graphical terminal:

```sh
env -u WAYLAND_DISPLAY MFALMION_WINDOW_SYSTEM=x11 VK_DRIVER_FILES=/usr/share/vulkan/icd.d/intel_icd.json WGPU_BACKEND=vulkan cargo run -p mfalmion-app --offline
```

- [ ] Open Game Review. Select e2; confirm e3/e4 markers. Play e2-e4; confirm Black to move. Test clicking and dragging, then an illegal move and wrong-side selection.
- [ ] Flip the board and move for the correct side; check coordinates, capture rings, and selected-square text. Resize to 760×540 and scroll to controls, then restore a larger window.
- [ ] New Game; play e2-e4, e7-e5, g1-f3, b8-c6, f1-c4, g8-f6, e1-g1. Confirm white king g1/rook f1. Undo restores e1/h1 and castling rights; castle again.
- [ ] New Game; play e2-e4, a7-a6, e4-e5, d7-d5, e5-d6. Confirm d5 pawn removed. Undo restores both pawns and the en passant opportunity.
- [ ] Verify promotion during play: position waits until one of four pieces is chosen. Verify cancel and Undo. FEN loading is not exposed; promotion fixtures are covered by unit tests.
- [ ] New Game; play f2-f3, e7-e5, g2-g4, d8-h4. Confirm checked-king highlight, Black wins, blocked further moves, and Undo resumes play.
- [ ] New Game; repeat g1-f3, g8-f6, f3-g1, f6-g8 twice. Claim Draw should become available without ending play automatically. Test the claim and Undo. In a new game, repeat four cycles without claiming to reach automatic fivefold.
- [ ] Check New Game clears pending actions/history. Navigate away and return; session remains in memory. Close normally and restart; the new process starts a new game.

## Storage and Git

Before this detailed 02B request: about 8.4 GB free; after compilation: about 8.2 GB free. Project target/: about 1.6 GB (rounded df/du). No cargo clean, cache deletion, system package installation, external archive/drive writes, or AI/CUDA changes occurred. No commits, pushes or remote configuration changes were performed for 02B. Stop here for user acceptance; 02C remains unapproved.

## Subsequent Git authorization

The user subsequently requested committing and pushing this work to GitHub. This supersedes the original no-commit/no-push instruction; native manual acceptance remains pending.
