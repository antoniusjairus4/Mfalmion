# Milestone 02C final audit

Date: 2026-11-16. Audited checkpoint: `5297402`, followed by the local fixes described here.

**PASS WITH WARNINGS — Manual verification required**

All final automated checks pass. No known blocking source-level defect remains after these fixes. The chess/session architecture is suitable for a future asynchronous Stockfish UCI adapter. Native desktop acceptance is still required. Milestone 03 was not started; no engine adapter was implemented. Nothing was committed or pushed during this audit.

## Scope and method

Read AGENTS.md, README, milestones 02B, 02B.1 and 02C; inspected the actual game, snapshot/coordinate adapters, SVG cache, board painter, animation controller, notation parser, review controller, file worker and app/sidebar integration. Existing compilation/test claims were not treated as proof of correctness. Three new regressions were first run against the pre-fix implementation: all three failed, reproducing contradictory PGN results, saturated FEN counters, and lost draw adjudication. The fixes and expanded suite were then verified.

No native GUI or OS graphical automation was launched. CPU-only egui unit tests do not connect to a display or initialize a GPU. One cleanup regression starts and immediately kills/reaps its own non-graphical `sleep` child; it does not target other processes. No GDM, virtual terminal, graphics driver, system package, external archive, Stockfish or Qwen operation was performed. No dependency or lockfile changes, cargo clean, release build, cache cleanup or external-drive modification occurred.

## Findings and fixes

| Severity | Finding | Resolution |
| --- | --- | --- |
| High | Switching games, returning from a variation, loading a new session or starting New Game could discard local work without warning. A completed asynchronous import could also replace work made while it was running. | All replacing actions pass through one controller guard. Local moves/claims, FEN analysis positions and variations require an explicit **Keep current session / Replace session** decision. Imported results remain pending until the decision. Cancel retains the current game. The warning is deliberately conservative and can still appear after exporting/copying. |
| High | PGN outcome callbacks accepted a winner contradicting checkmate, repeated result markers, or moves after a result. SetUp=0 could coexist with a custom FEN. | The visitor tracks termination, requires an explicit termination token, rejects contradictory/multiple results and inconsistent SetUp/FEN, and checks automatic endings against complete snapshots. Resignation/agreement results remain valid for nonterminal positions. An explicit `*` remains preserved as unknown, including a position whose board outcome is already known. |
| Medium | Navigating to the last position discarded a claimed draw; undoing a root-position claim left the record marked drawn and blocked continued play. | Local draw adjudication is retained alongside the timeline, restored at its final index and cleared correctly by root Undo. Earlier positions retain their original chess state. Result/status and board input gating now agree. |
| Medium | shakmaty's FEN parser saturates oversized counters. A syntactically huge halfmove value was accepted with a changed value. | Validate both counters as bounded unsigned 32-bit integers before library parsing; require a positive fullmove number. Limit FEN input to 512 bytes before splitting on the UI thread. No silent counter clamping. |
| Medium | Extra whitespace in valid PGN header pairs could become part of the key, losing player identity or FEN/SetUp recognition. Empty headerless games and common numeric castling notation were also rejected by the preliminary checker. | Trim visitor header keys, accept horizontal whitespace after `[`, and validate numeric castling without changing the original imported text. Preserve empty games with a result token. Reject `%` escape syntax outside the beginning of a line so the lexical checker cannot hide unvalidated text. |
| Medium | A 16 MiB byte limit did not bound the much larger indexed snapshot allocation. | Retain 16 MiB and add 200,000 total snapshots per import plus 4,096 plies per game. Both parsing and local append return readable limit errors. File size is checked before reading, and reads remain bounded if the file grows. |
| Medium | File paths were decoded only as UTF-8 and all trailing newlines were stripped, changing valid Linux filenames. Replacement confirmation interpreted path text as markup. | Decode Unix path bytes losslessly and remove exactly zenity's one output LF. Reject empty/NUL paths. Use literal confirmation text with `--no-markup`. File paths are passed as arguments/Path values, never shell source. |
| Medium | File workers could remain pending after disconnect; thread creation failure could panic; closing during a native dialog did not explicitly own/reap the child. | Use fallible named worker creation, propagate disconnection/spawn/dialog errors, and retain a task-owned child handle. Dropping the task cancels and kills/reaps its own dialog. Reject nonregular import selections. Preserve existing output permissions on confirmed replacement and log unexpected temporary-file cleanup failures. |
| Low | Finished recorded games could still show a drag ghost even though the controller refused the move. The selected import index could describe the previous game after loading standalone FEN/New Game. | One playability predicate controls both draggable pieces and accepted input; selected import identity is optional and reset for independent sessions. Final status includes the recorded result. |

The live controller previously re-enumerated legal moves and compared FENs to infer which move it had just played. It now consumes the exact validated `Move` identifier exposed by `Game`, stores typed identifiers beside SAN, and uses checked timeline append. If append fails (including a limit), it restores the indexed authoritative state and reports the error. This removes an unnecessary inference and protects board/timeline synchronization. The old promotion test fixture was updated to play through this same controller instead of changing only its private game and leaving its record behind.

## Chess, rendering and replay assessment

- shakmaty **0.30.1** remains the move legality authority. No independent move generator, rules replacement or graphics dependency was added.
- Castling still maps king destinations c/g onto shakmaty's king/rook representation. Both castling directions remain covered by the no-pending-promotion regression. En passant, captures, promotion, king safety, rights and counters retain existing tests.
- Renderer snapshots are read-only adapters from the authoritative board; orientation projection is independent of chess state. Cached Cburnett artwork and green/cream squares are unchanged. Temporary animation/drag images are presentation state, not another game board.
- Navigation installs indexed full `Chess` snapshots and restores the entire repetition prefix. Undo preserves the forward continuation. Restoring counters/rights/EP and repetition availability is tested. Claimed draws are now preserved separately from board state.
- Game and record agree on automatic fivefold and 75-move results, with checkmate taking precedence; appending/PGN moves after an automatic ending are rejected. Threefold and 50-move claims remain explicit.
- First/Last jumps skip intermediate movement; adjacent movement uses the existing 200 ms transition. Castling moves both pieces, reverse promotions restore pawns, and reverse captures restore the captured piece immediately. Rapid replacement drops old tracks. Flip cancels motion without changing the position. Drag settling remains 80 ms and invalid snap-back 120 ms.
- The scrollable SAN list, move clicks and keyboard gate were inspected. Keyboard navigation is suppressed while a replacement decision, promotion or text focus owns input. Physical keyboard/focus behavior still needs manual acceptance.
- Explicit local variations preserve their source continuation. A second branch or game/session switch now warns before replacing local work. This remains a single active local branch workflow, not a persistent variation tree.

## PGN/FEN and resource policy

Standard headers, player names, optional TimeControl/ECO/Opening, escaped values, comments, nested variations, SAN and multiple games are tested. Comments/variations remain preserved in the shared original document string, but normalized mainline export does not re-emit annotations/RAVs. The entire batch is parsed/validated before installation; invalid import leaves the previously selected game, selected ply, original games and FEN unchanged.

PGN imports now require a termination token (`*`, `1-0`, `0-1`, `1/2-1/2`). Incomplete/truncated text is reported rather than silently accepting a partial game. Only standard chess is supported. FEN without SetUp remains accepted as a common import convention, but a contradictory explicit SetUp is rejected.

The **16 MiB** policy is sufficient for selected games and many ordinary 729-game batches, but is not a guarantee about the user's uninspected archive. It allows about **22.5 KiB per game** for 729 games. An in-memory synthetic 729-game fixture with approximately 16,000 comment bytes per game (about 11.2 MiB total) imports successfully. This tests byte/batch handling, not the actual archive or typical game length. No external archive was loaded or examined.

The additional 200,000-snapshot budget permits an average of about 273 plies per game across 729 games after initial snapshots; the separate per-game maximum is 4,096 plies. Exceptionally long or heavily annotated archives must be split into selected smaller files. Future synchronization/library ingestion should stream batches rather than raise these limits indiscriminately. Parsing, dialogs and disk IO stay on workers. FEN checks and active-game export/copy are bounded; restoring the repetition prefix and rendering the move list remain linear in the selected game's size.

Export still writes/syncs a temporary file in the chosen directory. A new destination is published atomically without clobbering; an existing one requires explicit confirmation before rename. Rejected replacement and collision tests preserve original contents. Symlink selections follow normal OS behavior: import reads the target; confirmed export replaces the selected directory entry. No path expansion, home scanning or arbitrary command execution was introduced.

## Future asynchronous Stockfish compatibility

| Required input | Existing access |
| --- | --- |
| Current position / side to move | `Game::authoritative()`, `notation::fen()`, library turn, and `Game::side_to_move()` |
| Full indexed timeline | `GameRecord.positions`, index zero initial and cursor-selected complete state |
| Exact move identifiers | `GameRecord.played_moves: Vec<shakmaty::Move>`, aligned with canonical SAN; standard-castling UCI conversion is tested |
| Initial position plus played prefix | Initial snapshot FEN plus `played_moves[..cursor]`; retains repetition history for a future UCI `position ... moves ...` command |
| Metadata and original source | Header dictionary, original PGN Arc, result and local adjudication |
| Selected game/position | Review's optional imported source index, active record/branch and cursor |

A future adapter can receive an owned immutable request from the review controller and return results through a channel. It will need ordinary request revision/cancellation checks for stale results. Workers must not mutate the board or egui state directly. These are future adapter responsibilities; the board renderer does not require rewriting. No process launcher, UCI parser, evaluation model or engine networking was added here.

## Verification

Final commands, all **exit status 0**:

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo check --workspace` | PASS |
| `cargo test --workspace` | PASS — **72 passed, 0 failed, 0 ignored** |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS — no warnings |
| `git diff --check` | PASS |

The final suite includes all original 56 tests plus 16 audit tests: conflicting/premature results, bounded FEN counters, complete claimed-draw replay and root Undo, invalid-import preservation, guarded replacement actions, literal/non-UTF-8 filenames, invalid UTF-8/nonregular inputs, child cleanup, empty/numeric-castling PGN with typed UCI identifiers, resource budgets, automatic draw consistency, transactional invalid append, header whitespace/escape lines, synthetic 729-game import, and all four promotion hit targets for both colors/orientations. Existing export overwrite regression additionally checks permission preservation.

Three deliberately added pre-fix regressions initially failed. One existing test's out-of-sync fixture and intermediate compile issues were corrected before final verification; no warnings or failed tests remain. No native GUI verification is claimed.

## Disk and modified files

Before audit verification: **7.3 GiB free**, target approximately **2.1 GiB**. After final checks: **7.2 GiB free** (approximately **7.7 GB decimal**), target approximately **2.2 GiB**. Existing build artifacts were reused; no cleanup or system installation was performed.

Modified: `board/game.rs`, `notation.rs`, `file_dialog.rs`, `ui/game_review.rs`, README. Created: this audit report. Cargo.toml, Cargo.lock, artwork/licenses, graphics policy and unrelated pages are unchanged. Git changes remain local and uncommitted.

## Manual GUI acceptance still required

Run explicitly from a graphical terminal:

```sh
cd '/home/jairus/Antonius Jairus/Projects/Mfalmion'
env -u WAYLAND_DISPLAY MFALMION_WINDOW_SYSTEM=x11 VK_DRIVER_FILES=/usr/share/vulkan/icd.d/intel_icd.json WGPU_BACKEND=vulkan cargo run -p mfalmion-app --offline
```

- [ ] Verify physical click/drag movement, illegal-drop snap-back, resizing and flipping; check crisp pieces and no stranded/duplicate images during rapid replay.
- [ ] Check first/previous/next/last, move-list clicks/scrolling and Left/Right/Home/End. Verify text editing and replacement/promotion dialogs suppress navigation appropriately.
- [ ] Check castling both ways, captures, en passant and forward/backward promotion animation.
- [ ] Load white promotion FEN `7k/P7/8/8/8/8/8/7K w - - 0 1` and black promotion FEN `7k/8/8/8/8/8/p7/7K b - - 0 1`. Test all choices, Escape/outside cancellation, board blocking, clicking/dragging and both orientations.
- [ ] Import selected single/multi-game files, inspect metadata, copy FEN/PGN, and paste back. Verify malformed input leaves the selected game and position intact.
- [ ] Make local moves/variations, then attempt game switching, New Game, FEN/PGN replacement and return-to-original. Cancel first and verify the work remains; export/copy and explicitly confirm replacement.
- [ ] Test native file cancellation, existing-path overwrite rejection/confirmation and export/reimport. Close the app while a native dialog is open and confirm that dialog exits too.
- [ ] Claim a repetition/50-move draw, navigate away/back and check the claim; test root-position claim Undo with `r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 100 51`.
- [ ] Verify surrounding pages, normal X11 close and clean shutdown.

## Remaining limitations and readiness

Native X11 rendering/input, desktop focus, zenity behavior and clipboard remain unverified in this audit. The prior Wayland surface issue is unchanged; use the documented X11/Intel Vulkan command. Standard-chess only, bounded in-memory imports, nonpersistent local branches, omitted annotations in normalized export, immediate reverse-capture restoration and conservative arbitrary-dead-position detection remain documented limitations. Exact snapshot representation has internal nonempty/index invariants; the UI clamps navigation and the parser constructs records transactionally, so untrusted input does not select unchecked indices.

The final decision is **PASS WITH WARNINGS — Manual verification required**. Complete the manual checklist and obtain explicit user approval before Milestone 03. This audit stops here.
