# Milestone 06.1 — Game Review presentation and integration

Implemented for manual visual acceptance. The user reports 151 completed analyses and has accepted Milestone 06 runtime behavior. This milestone does not start Milestone 07, reanalyze games, change classification thresholds/accuracy formulas, alter stored evidence, migrate/reset the database, or commit/push changes. No native GUI or desktop automation was launched. The real application database was not opened during development; new database tests use temporary fixtures.

## Root cause

The former `app.rs` placed the whole Game Review in a single vertical scroll area. `GameReview::details` rendered the entire interactive engine panel before replay controls and move history, in a fixed 240-point sidebar. The saved batch review was appended by the app after the whole board/details layout. Thus longer engine output displaced navigation, and classifications/accuracy appeared below the main review instead of alongside it. The move-history renderer did not consume persisted classifications at all.

Additionally, the old review query selected only analyses matching the current batch preferences. Changing those preferences could hide a valid completed review created with earlier settings. This is an integration condition found in the code; no assumption was made that the user's actual saved preferences had changed.

## Layout

Game Review now has three regions:

1. A fixed top area: title, player names/result, First/Previous/Next/Last, Flip, position/move number, keyboard hint and critical-move navigation. It is outside all scroll regions.
2. Left: the existing square board, compact authoritative game status and promotion/interaction notices.
3. Right: saved Game Analysis Summary, classified move history, Selected Move Analysis and collapsible live interactive Stockfish output.

`ui/review_layout.rs` calculates a nominal 60:40 board/sidebar split. Board size is constrained by available height, then surplus width goes to the sidebar instead of empty space. The right region is wider than the former 240-point implementation at all four requested desktop sizes. Coordinate margins remain accounted for, and the existing SVG texture cache handles the new board sizes. No artwork, animation duration, orientation mapping or legal rules were changed.

Review-specific control spacing is compact and local. The surrounding application theme/sidebar are preserved. Names and long lines truncate with full-text tooltips, while normal history text and live principal variations wrap. On smaller review regions, classification counts collapse and history uses a smaller height when live details are open. Accuracy remains visible. Critical controls wrap instead of overflowing horizontally.

PGN/FEN import/export, metadata, imported-game selection, local variations, new game, undo and draw controls remain available through **Game tools…**. Existing library save/position-cache controls are in **Library storage / position cache…**. These optional windows do not occupy permanent board space or displace replay navigation.

## Independent scrolling

The page-level Game Review scroll was removed. Move history has its own bounded vertical scroll area. Live engine controls/output have a separate bounded vertical scroll area when expanded. Optional metadata/session tools have one independent scroll region in their window. The existing navigation sidebar scroll is separate as before. There is no scroll area surrounding the board/history/engine combination, so normal history scrolling leaves the board and toolbar stationary.

The live section remains usable with the existing Start/Stop, requested depth, MultiPV, engine settings, score, evaluation bar, best move and variations. PV text uses a minimum explicit 13-point size. Its header identifies it as live interactive analysis and displays the requested depth; the panel still displays actual search depth separately. A completed saved batch review shows its own recorded engine/depth/MultiPV. Live output never changes the saved classification model.

## Read-only saved review loading

`storage/review.rs` adds a presentation query using existing tables and validation. It selects the latest completed review for the supported classification algorithm using that review's recorded configuration, independently of today's batch defaults. It validates the game/profile/fingerprint and summary, then reads the requested position's persisted evaluation at the classification depth (or the appropriate terminal/base budget). It never writes to the database.

The existing bounded background worker accepts a `Review` command. The UI does not execute SQL or reparse PGNs on its frame thread. Responses include a profile/game/fingerprint/ply key and share the summary through `Arc`. Rapid cursor changes cannot attach another game's summary or another position's score to the selected move. Same-game summary data can update the selected classification immediately; while the selected position's score is being fetched, the card says it is loading.

Missing, pending, paused, failed, cancelled, unsupported and corrupt review states have explicit messages rather than invented accuracy or classifications. A prior completed review stays available during a new incomplete attempt and is labeled as prior evidence. Local branches, unsaved sessions, another active profile and stale fingerprints cannot inherit a stored mainline's classifications.

## Visible classifications and selected move details

The summary displays **Mfalmion Estimated Accuracy** separately for White and Black, eligible/excluded coverage, and counts for Brilliant, Great, Best, Excellent, Good, Inaccuracy, Mistake, Blunder and Miss. All values come from validated SQLite records; absent category counts in a completed summary mean zero. No summary means an empty/incomplete state, not zero accuracy.

Each SAN history entry includes its primary classification text. Colors are cyan for Brilliant, blue for Great, green for Best, light green for Excellent, neutral for Good, yellow for Inaccuracy, orange for Mistake, red for Blunder and purple for Miss. Text/tooltips make the classification understandable without color. The selected entry uses the existing selected-widget highlighting.

Clicking history uses the existing `navigate` method, preserving authoritative positions and replay animation. It updates classification/details immediately and requests the matching saved position evaluation asynchronously. The selected card shows played SAN, classification/confidence/depth, expected-score loss, best engine move, best continuation, White-perspective position evaluation with its score bound, and explanatory tags. Best-move/PV UCI identifiers are converted to readable SAN using the existing legal PV decoder. Long selected-card lines use full-text hover tooltips. Missing evidence is labeled **Not analyzed**.

Previous/Next mistake/blunder filters only those two persisted primary categories. Previous/Next critical move includes Brilliant, Great, Miss, Mistake and Blunder. Navigation is strictly before/after the current ply; buttons disable when no target exists or during a pending promotion. There is no classification inference from live scores.

## Data protection

No schema version, migration SQL, engine evidence, analysis settings, thresholds or accuracy formulas changed. Existing games, summaries and checkpoints are preserved. A test records SQLite `total_changes()` before and after the new query and confirms no writes. Tests also confirm saved results remain visible when batch preferences change, and that wrong profiles/stale fingerprints are rejected. No real account reanalysis or engine smoke was run for this presentation-only change.

## Verification

Final results on 2026-11-16:

| Command | Result | Exit |
| --- | --- | --- |
| `cargo fmt --all -- --check` | Pass | 0 |
| `cargo check --workspace` | Pass | 0 |
| `cargo test --workspace` | 249 passed, 0 failed, 3 ignored; 252 discovered | 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | Pass, no warnings | 0 |
| `git diff --check` | Pass | 0 |

The three opt-in tests remain skipped: real interactive Stockfish, real batch Stockfish, and live Chess.com HTTP. Fifteen new passing tests cover completed review loading, recorded settings despite preference changes, correct accuracy/counts/evaluation, read-only behavior, missing/incomplete/incompatible states, profile/fingerprint isolation, prior completed review preservation, asynchronous result delivery, classification/ply mapping, selection/animation, critical navigation/boundaries, saved-versus-live settings, layout state preservation and requested sizes.

Pure geometry tests cover 1920×1080, 1440×900, 1366×768 and 1280×720 after accounting for application chrome, plus narrower content widths. CPU-only egui layout tests use the application theme, a loaded review and live details both collapsed and expanded, checking horizontal and vertical bounds at all four desktop sizes. These tests open no native window, do not interact with the desktop and do not start an engine. One such test caught and prevented wrapped badge-frame padding from causing horizontal overflow.

Before compilation, free SSD space was **7,620,362,240 bytes** (about 7.10 GiB). The post-build reading was **7,606,394,880 bytes** (about 7.08 GiB). No cleanup, package installation, graphics changes or Qwen launch occurred.

## Manual visual acceptance checklist

Launch manually:

```sh
cd '/home/jairus/Antonius Jairus/Projects/Mfalmion'
env -u WAYLAND_DISPLAY MFALMION_WINDOW_SYSTEM=x11 VK_DRIVER_FILES=/usr/share/vulkan/icd.d/intel_icd.json WGPU_BACKEND=vulkan cargo run -p mfalmion-app --offline
```

- Open a completed game from the 151-game library. Confirm saved engine/depth, White/Black accuracy and category counts are visible without scrolling past live analysis.
- Compare displayed summary values with the existing saved review. Confirm no batch starts automatically.
- Click White and Black moves; verify board position, selected classification, alternative/PV and saved score track the selected ply. Check initial-position and missing-analysis states.
- Use First/Previous/Next/Last, Left/Right, Home/End and Flip. Confirm the toolbar stays in place and animations/orientation work.
- Navigate both critical filters; verify boundaries disable and targets correspond to saved categories.
- Scroll history with the pointer over it. Confirm the board/toolbar stay stationary.
- Expand live Stockfish, start it explicitly if desired, and scroll long variations. Verify live settings/actual depth are distinct from saved batch depth and classifications remain unchanged.
- Resize to 1920×1080, 1440×900, 1366×768 and 1280×720 at the desktop's actual scaling. Check square/crisp board, usable sidebar, wrapping and compact count controls.
- Test Game tools: PGN/FEN, metadata, imported-game switching, local variation/return, undo/new game and promotion. Confirm a local branch does not reuse mainline classifications.
- Check another profile or a non-analyzed stored game. Confirm useful empty states and no cross-profile evidence.

Native appearance, pointer-wheel routing on the installed desktop, display scaling and live-engine runtime behavior still require manual confirmation. Compact count groups may need expanding; full selected-move continuations/tags are available on hover. Very short/minimum-size windows necessarily have a smaller board and live output viewport. No full Dashboard or Milestone 07 features were added.

## Files and Git

Created:

- `crates/mfalmion-app/src/storage/review.rs`
- `crates/mfalmion-app/src/ui/review_layout.rs`
- `docs/milestone-06-1.md`

Modified:

- `crates/mfalmion-app/src/analysis/tests.rs`
- `crates/mfalmion-app/src/analysis/worker.rs`
- `crates/mfalmion-app/src/app.rs`
- `crates/mfalmion-app/src/storage/mod.rs`
- `crates/mfalmion-app/src/ui/batch_panel.rs`
- `crates/mfalmion-app/src/ui/engine_panel.rs`
- `crates/mfalmion-app/src/ui/game_review.rs`
- `crates/mfalmion-app/src/ui/mod.rs`

The starting working tree was clean. Final changes are unstaged: eight modified tracked files and three new files. No commit or push was performed. Stop at Milestone 06.1 for manual acceptance.
