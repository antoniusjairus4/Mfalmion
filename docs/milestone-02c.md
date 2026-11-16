# Milestone 02C — PGN/FEN, replay and on-board promotion

Implemented 2026-11-16. Manual desktop acceptance is pending. No native GUI application or graphical automation was launched. No engine, AI server, external archive, system package, display manager or GPU configuration was touched. No commit or push was performed. Milestone 03 has not begun.

## Parsing and reusable game data

`notation.rs` holds a GUI-independent `GameRecord`: standard and optional metadata in a header dictionary (including White/Black identities, Event, Site, Date, Round, Result, TimeControl, ECO and Opening), original imported PGN, canonical SAN moves, complete `Chess` snapshots, and result. The representation can be consumed by future API ingestion or persistence without depending on egui. No fetching/database code is present; future target `Antonius-Jairus-R` is not contacted.

The maintained [pgn-reader 0.29.0](https://docs.rs/pgn-reader/0.29.0/pgn_reader/) visitor supplies headers and mainline SAN. It depends on shakmaty 0.30; Cargo resolves the existing exact **0.30.1**, without a second rules version. pgn-reader is GPL-3.0-or-later, matching the existing rules library. This adds one locked package and reuses btoi/memchr. No large GUI library was added.

The reader intentionally tolerates malformed tokens. A preliminary lexical check rejects malformed SAN tokens, unterminated tags/comments/variations and unmatched delimiters. Each mainline SAN is resolved against the current authoritative shakmaty position, then played and canonicalized. Illegal or ambiguous moves and false check/mate suffixes produce game/ply errors; no moves are invented or repaired. Variations are skipped by the visitor; comments and NAGs do not change the mainline. Multiple games are returned together; any failure rejects the entire import before replacing the active session. Duplicate tags, unsupported variants and conflicting result declarations are rejected.

Every imported game shares an `Arc<str>` of the original **whole imported document**. This preserves comments/variations verbatim without copying the input for each game. Normalized export contains the active mainline and metadata, not the original comments/variations. Standard roster headers are emitted first; additional headers, escaped values, result and custom SetUp/FEN are retained. Movetext wraps at 80 columns. SAN comes from validated library moves. Copy PGN uses egui's native clipboard output.

## FEN and position timeline

FEN loading requires all six fields and a positive fullmove number, then uses shakmaty's strict standard-chess position validation. Invalid input leaves the active session untouched. A successful standalone FEN starts a new analysis/replay session at index zero. Display/copy uses `EnPassantMode::Always` so the complete FEN includes the EP field, counters, turn and rights.

Timeline index zero is the initial position; index n is the complete state after n plies. Navigation installs the indexed `Chess` snapshot and restores its prefix as the exact repetition history. No PGN is reparsed. Restoring that prefix currently clones O(n) snapshots; this is appropriate for individual games, but not a streaming archive browser. Chess equality itself ignores clocks for repetition, while the stored snapshots retain them.

The scrollable SAN panel shows move numbers, White/Black columns, selected ply and game result. First/Previous/Next/Last and move clicks navigate. Left/Right/Home/End apply when egui does not own keyboard text input and no promotion is pending. Undo navigates backward without destroying the continuation.

Imported mainlines are read-only. To play from an imported or historical position, the user must explicitly choose **Create local variation from here**. The branch clones the prefix; the source continuation is retained. The panel labels the local variation and offers **Return to original mainline**. Imported originals also remain selectable in the game dropdown, including a single-game import. Export/copy serializes the active branch/mainline. This is a simple local branch workflow, not a nested variation editor. New Game is an explicit fresh-session action.

## Promotion and rendering

`board/view.rs` paints four native egui hit targets over the board, using the existing cached Cburnett textures for the promoting side. Queen, Rook, Bishop and Knight form a vertical strip anchored to the destination file, with its rectangle clamped inside the visible board. Orientation projection handles either board orientation and either promoting color.

The authoritative move stays unplayed until a choice is clicked. Underlying square interactions use hover-only sensing while pending. Escape or clicking outside cancels without changing the position. Click and drag moves reach the same promotion controller. The side-panel text buttons were removed. No HTML, WebView, Unicode piece glyphs or new artwork pipeline was introduced.

Inspection found no existing castling/promotion classification fault: only legal candidates with `Move::promotion().is_some()` request promotion. Castling continues to adapt the library's rook-square representation to normal c/g king destinations. A controller regression verifies both kingside and queenside castling execute, animate both pieces and never set pending promotion.

The green/cream board, dark shell, bundled licensed SVGs, shakmaty version, wgpu renderer and existing draw rules are preserved.

## Animation integration

Adjacent replay transitions reuse the existing elapsed-time 200 ms animation. Reverse transitions match removed/arriving pieces, including promoted pieces returning to pawns. Castling animates king and rook in both directions. Captured pieces disappear immediately forward and reappear immediately backward, while the moving piece animates. En passant follows snapshot state. New navigation replaces old tracks, preventing stranded/duplicated animated destinations. First/last non-adjacent jumps skip motion. Drag settling remains 80 ms; invalid-drop snap-back remains 120 ms. Authoritative snapshots are installed before painting.

## Native files and resource safety

Import/Export buttons invoke the already installed `/usr/bin/zenity`. Dialogs, bounded file reads, parsing and writes run on a worker thread and notify egui to repaint. Pasted PGN parsing also uses a worker. If the native dialog is unavailable, a readable error offers paste/copy as fallback. Cancellation does not change the session. Files are not enumerated or imported without user selection. Inputs above 16 MiB are rejected.

Export writes and syncs a temporary file in the selected directory. Existing paths require a separate explicit native replacement confirmation before atomic rename. New paths use atomic no-clobber hard-link publication; a race creating the destination fails rather than overwriting. Temporary files created by the operation are removed. There is no automatic home/archive scanning, package installation or external-drive modification.

Before compilation: approximately **7.8 GiB** free. After final checks: approximately **7.3 GiB** free (about **7.8 GB decimal**), target/ approximately **2.1 GiB**. A broad Cargo fetch also downloaded cached cross-platform dependency sources; this was unnecessary for the Linux build. No caches/artifacts were deleted and no full clean/release rebuild was performed.

## Automated verification

Passed:

```sh
cargo fmt --all -- --check
cargo check --workspace --offline
cargo test --workspace --offline
cargo clippy --workspace --all-targets --offline -- -D warnings
git diff --check
```

**56 tests passed, zero failed.** Coverage includes standard headers/SAN, multiple games, illegal SAN/token/FEN rejection, comments/nested variations/NAGs, castling, en passant, all promotion choices, custom FEN, state/counter/rights/EP preservation, indexed forward/backward/first/last navigation, repetition prefix restoration, imported-mainline preservation and explicit local branching, promotion rectangle placement for both orientations at three sizes, promotion cancellation and blocked board input, both castling directions never opening promotion, exported PGN round trips, overwrite protection, and rapid reverse/forward special-move animation replacement. Existing rules, mapping, SVG-cache and CPU-only egui controller tests remain passing.

Tests use pure state or CPU-only egui contexts. They open no native window and connect to no display/GPU. They do not establish native desktop visual acceptance.

## Limitations and manual acceptance

- Native dialogs, clipboard, physical mouse/keyboard, resizing and visual appearance require manual acceptance; no GUI was launched.
- Standard chess only, 16 MiB imports, all imported games/timelines held in memory; no persistent game library or huge-archive streaming.
- Original document text preserves annotations, but normalized mainline export omits comments/NAGs/variations. Local variations are active-session branches, not a PGN RAV editor, and are lost when switching away unless exported.
- Reverse captures restore the captured piece immediately rather than animating its appearance. Large jumps intentionally skip motion.
- The prior conservative insufficient-material/dead-position limitation remains. No engine, SQLite, Chess.com or Qwen integration exists.
- Linux native file dialogs require the existing zenity executable. Other platforms can use paste/copy; native dialog backends for them are not implemented.

Run manually from a graphical terminal:

```sh
cd '/home/jairus/Antonius Jairus/Projects/Mfalmion'
env -u WAYLAND_DISPLAY MFALMION_WINDOW_SYSTEM=x11 VK_DRIVER_FILES=/usr/share/vulkan/icd.d/intel_icd.json WGPU_BACKEND=vulkan cargo run -p mfalmion-app --offline
```

- [ ] Open Game Review; verify dark panels, green/cream board and crisp cached pieces. Resize and flip.
- [ ] Import a small selected PGN with several games; switch games and inspect names, metadata, SAN columns and result. Cancel a file dialog and verify nothing changes.
- [ ] Test Previous/Next, First/Last, move clicks, Left/Right and Home/End. Confirm typing in PGN/FEN inputs does not navigate.
- [ ] Replay captures, en passant, both castling directions and promotions forward/backward; navigate rapidly and flip/resize during motion. Confirm no stranded/duplicate pieces.
- [ ] Verify imported moves are read-only. Navigate backward, explicitly create a local variation, play a different move, return to the original and verify its continuation survives.
- [ ] Copy/load a valid six-field FEN; verify turn, rights, EP and clocks. Submit invalid FEN and PGN and confirm the current board/game remains intact.
- [ ] Load `7k/P7/8/8/8/8/8/7K w - - 0 1`. Move a7-a8 by click, choose each piece in separate sessions, test Escape/outside cancellation and board blocking. Repeat by drag and after flipping.
- [ ] Load `7k/8/8/8/8/8/p7/7K b - - 0 1` and repeat black promotion in both orientations. Verify chooser stays on the board.
- [ ] Load `r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1`; castle either direction. Confirm no promotion chooser and both pieces move.
- [ ] Copy/export PGN, reimport, compare SAN/result/FEN. Try an existing output path, reject replacement and verify its content is unchanged; then confirm replacement.
- [ ] Verify New Game and Undo, other five page navigation, normal window close and clean exit.

Stop at 02C for explicit user acceptance before Milestone 03.
