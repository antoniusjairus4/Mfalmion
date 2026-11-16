# Milestone 03 — Native Stockfish 17.1 live position analysis

Implemented 2026-11-16 after the user's explicit acceptance of Milestone 02C. Milestone 03 only: no SQLite, Chess.com synchronization, account archive/batch analysis, Qwen coaching, engine play or Milestone 04 work. No commit or push was performed. Existing source, PGN fixtures, board artwork, licenses, graphics policy, rules and animations are preserved.

## Executable and protocol verification

The existing `/usr/games/stockfish` was verified before integration: it identified itself as **Stockfish 17.1**, advertised Threads/Hash/MultiPV spin options, answered `uciok`/`readyok`, and exited with status 0 after `quit`. No engine binary was downloaded, copied or compiled. The executable is an external GPLv3 Stockfish installation; this change adds no engine binary distribution or new Cargo dependencies.

Protocol behavior was checked against the [Stockfish 17.1 UCI implementation](https://github.com/official-stockfish/Stockfish/blob/sf_17.1/src/uci.cpp). The second final smoke fixture is the castling-rich middlegame position also used by [Stockfish's standard perft tests](https://github.com/official-stockfish/Stockfish/blob/sf_17.1/tests/perft.sh). Only the installed binary is executed at runtime.

## Architecture and ownership

`engine/` is independent of egui:

- `mod.rs`: validated settings, immutable requests, generation-tagged events, status and bounded request/result channels.
- `protocol.rs`: UCI handshake/option/info/bestmove parsing, White-normalized score types, complete MultiPV snapshots and UCI-to-SAN decoding.
- `process.rs`: direct `std::process::Command` launch, bounded stdin/stdout transport, partial-line assembly, UTF-8/line-size checks, and bounded process teardown.
- `worker.rs`: one authoritative UCI/search owner, handshake/readiness, option negotiation, request coalescing, stop acknowledgement, process reuse, error recovery and shutdown.
- `tests.rs`: deterministic protocol/process tests and an explicitly ignored real-engine smoke test.

`ui/engine_panel.rs` owns presentation/settings/current request identity. Game Review supplies a clone of the current authoritative `Chess` and a session generation. Normal moves, indexed navigation, FEN/PGN installation, game switching, New Game and local branching invalidate old results immediately. Board flipping/resizing changes presentation only and does not restart an unchanged position. Navigation to the already-selected index also avoids an unnecessary restart.

The worker launches only after **Start Analysis**. One healthy process is reused across moves and settings changes. A changed executable path replaces the old process after teardown. Thread/hash/MultiPV settings are sent only when changed. `ucinewgame` is sent on initial connection and session changes; `isready` is required after configuration/new-game setup and before `position fen ...`/`go`.

All engine paths go directly to `Command::new(path)` without a shell or command-string interpolation. Missing, inaccessible, non-executable or invalid binaries result in a recoverable Error. Required options must be advertised with compatible ranges before they are set; unsupported required options produce an error. Optional `UCI_AnalyseMode` is set only when advertised as a check option.

## Asynchronous transport and lifecycle

The request channel has one slot: new work replaces queued obsolete work under a short receiver lock. Lower generation IDs cannot overwrite newer queued requests, including requests returned to the queue after startup. One active search is allowed per engine. Navigation sends `stop`, discards old info until its `bestmove`, and then starts only the latest queued request. Startup/readiness recheck coalesced work before any search begins.

Results use a bounded channel retaining at most one latest event of each kind (status, identity, snapshot, bestmove, error). This bounds backlog even when Game Review is hidden. Each result carries its request ID, and the UI rejects all noncurrent IDs. It also clears evaluations/best moves immediately when the position/session changes. The worker calls a generic wake callback; the UI supplies egui's thread-safe repaint request. No egui object is mutated by the engine worker and no busy UI polling loop is required for analysis.

Each process has one bounded writer queue (16 commands) and stdout queue (256 messages). Small reader/writer threads handle pipe IO; neither blocks egui. The reader assembles arbitrary partial reads up to a 64 KiB line limit, reports invalid UTF-8/EOF/IO failures, and does not allocate an unbounded line. Stderr is discarded instead of leaving an undrained pipe. Unknown/non-evaluation info is ignored; malformed numeric messages cannot update the snapshot. A bestmove is validated against the exact requested position before being shown.

Timeouts:

| Phase | Limit |
| --- | --- |
| UCI startup | 5 seconds |
| Readiness | 3 seconds |
| Active cancellation / bestmove acknowledgement | 2 seconds |
| Search | `go depth N movetime 30000`; independent 35-second watchdog |
| Quit | 500 ms grace, followed by kill/reap if necessary |

The search watchdog is checked regardless of whether the engine is silent or continuously emitting output. Tests use shorter deadlines for failure cases. On failure, the worker retires the child and reports the error; Start Analysis can retry using the same worker and corrected settings. Stop disables position following and clears the live display; a healthy process stays connected in Ready state for reuse.

Normal window close requests asynchronous worker shutdown, cancels the native close temporarily, keeps rendering a closing message, and finishes closing when the worker confirms teardown. It does not join/wait for UCI on egui. The worker sends stop/quit as appropriate, waits for exit within the grace period, and kills/reaps a nonresponsive child. Pipe channels are closed; IO threads are joined only when finished, with bounded cleanup waiting on the worker. The client destructor additionally signals kill without a UI-thread wait as an emergency backstop. Tests verify normal quit, ignored quit, cancellation failure, and dropping a client during an active search without leaving its process behind.

## Scores, MultiPV and SAN

Centipawn scores are converted from the root side-to-move convention to **White-positive** values. When Black is to move, centipawn signs are negated and lower/upper bound directions are exchanged. Mate scores have a distinct type storing winner and move distance, including mate zero as the side to move already being mated. They are not converted to centipawns. Bound scores have explicit ≥/≤ labels and are not presented as exact estimates.

Depth, seldepth, nodes, nps, elapsed milliseconds, MultiPV index and raw PV moves are parsed. Snapshot assembly requires all expected ranks from one depth and request. The expected rank count is reduced for roots with fewer legal moves. A completed depth's pending rows are removed, so a same-depth partial refresh cannot reuse old ranks as a new complete group. Lower-depth messages cannot replace a newer completed snapshot. A terminal position can publish its score without a PV.

Each displayed PV begins from a clone of the exact analyzed `Chess`. Each UCI move is converted to a legal library move before application, then rendered using canonical SAN with proper move numbers. Up to 12 half-moves are displayed. Castling, captures, en passant and promotion use shakmaty's conversions. An invalid PV stops at its valid prefix and shows a warning; it cannot change the game. Bestmove is display-only, not a move execution command.

## UI and settings

The compact analysis section is at the top of Game Review's existing side controls. It shows Offline/Starting/Ready/Analyzing/Stopping/Error, engine identity, evaluation, depth/seldepth, nodes/nps/time, bestmove and up to three SAN PVs. Native egui controls provide Start/Stop, depth, MultiPV, and expandable executable/thread/hash settings. **Apply settings** restarts an enabled analysis using validated draft values. Settings are in-memory only.

| Setting | Default | Valid range |
| --- | --- | --- |
| Executable | `/usr/games/stockfish` | Nonempty UTF-8 path, no NUL/newline, ≤4096 bytes |
| Depth | 16 | 1–32 |
| MultiPV | 3 | 1–3 |
| Search threads | 2 | 1–min(8, available parallelism) |
| Hash | 64 MiB | 16–512 MiB |

Limits apply before engine-specific option bounds. The app does not allocate all i7-12700H CPU threads or a large fraction of 15 GiB RAM by default. The existing outer scroll area keeps controls reachable on smaller windows; manual native layout acceptance is pending.

A **horizontal** evaluation bar was chosen to preserve the existing board geometry/animation/input layout. Its white portion uses `0.5 + 0.48 * tanh(cp / 400)` for centipawns; mate is decisive white/black. This is a bounded nonlinear display scale, not a calibrated probability or WDL conversion. White remains labeled on the left and Black on the right regardless of board orientation; bound estimates are labeled separately. No Unicode pieces, art changes or board redesign were introduced.

## Tests and real smoke test

All required checks pass:

```sh
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Normal test suite: **89 passed, 0 failed, 1 intentionally ignored**. The ignored case is only the real Stockfish smoke test. All existing 73 board/rules/PGN/FEN/replay/asset tests remain passing. New coverage includes handshake/options, cp/mate/side normalization/bounds, depth/node/time/MultiPV parsing, coherent snapshots and same-depth partial refreshes, UCI-to-SAN and special moves, resource limits, latest-request coalescing, stale result rejection, position/session changes, stop/counter state, partial wire reads, missing/invalid paths, EOF, malformed/oversized/non-UTF-8 output, startup/readiness/cancellation watchdogs, chatty output, process reuse, graceful/forced quit, emergency drop, recovery and child reaping.

Offline process tests use the deterministic `tests/fixtures/mock_uci.py` with the already installed Python 3 interpreter. It is a test-only protocol fixture, not an application frontend or production dependency. Tests directly execute temporary fixture paths (including spaces), use no network/native window, and clean up only their own generated files/processes. No system package was installed.

After unit tests passed, the explicitly requested low-resource real smoke test was run:

```sh
cargo test --workspace --offline engine::tests::real_stockfish_smoke -- --ignored --nocapture
```

Result: **1 passed, 0 failed**. It verified Stockfish 17.1 handshake, option setup, initial position and the following known middlegame fixture, depth-limited search, complete three-line MultiPV output at depth 6, legal bestmove responses, reuse of the same PID, quit and process reaping:

```text
r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1
```

Smoke settings: **1 thread, 16 MiB hash, MultiPV 3, depth 6**. A recorded run reported `d4` for the start and `Bxa6` for the middlegame; these are shallow, run-specific engine outputs, not asserted optimal answers or user-game annotations. Both complete snapshots and best moves were validated. The test asserts that its owned PID no longer exists after shutdown; `pgrep -a -x stockfish` afterward returned no processes. No Stockfish child remains from verification.

No native Mfalmion GUI was launched or graphically automated for this milestone. Earlier GUI launches in earlier tasks do not constitute Milestone 03 acceptance.

## Performance, limitations and storage

The process is reused, unchanged thread/hash options are not reapplied, requests/results are bounded/coalesced, and only complete snapshots reach the panel. The existing SVG texture cache and 200/80/120 ms animation timings are untouched. These are design properties, not measured native FPS/latency claims.

Limitations:

- Native GUI layout, focus, physical input, live navigation and close behavior still require manual acceptance.
- Settings are volatile. Executable configuration accepts a direct path, not shell commands, argument strings or a shell wrapper supplied by the app. Generic UCI engines must support the three required spin options; other variants/options are not implemented.
- Analysis sends the exact current FEN, not an initial-position-plus-move-history command. Thus engine repetition assessment does not include earlier game history; Mfalmion's authoritative repetition/draw rules remain unchanged and separate. Recorded results/draw claims are not engine evaluations.
- A bounded search may finish below the requested depth. The last complete MultiPV depth remains visible; an incomplete newer depth is not mixed in. PV display stops at 12 half-moves.
- Non-evaluation info strings/stderr are not exposed as a full engine console. EOF/timeouts/transport failures are reported. No WDL model, batch analysis, evaluation persistence, auto-move/play, database, Chess.com or Qwen integration is added.
- The earlier Wayland issue remains outside this milestone; use the accepted X11/Intel Vulkan launch.

Disk before compilation: approximately **7.2 GiB free**. After final checks: approximately **7.2 GiB free**; `target/` approximately **2.2 GiB**. Existing artifacts were reused; no cargo clean, release build, package installation, external archive operation or destructive cleanup occurred. Cargo.toml/Cargo.lock are unchanged.

Created: `engine/{mod,protocol,process,worker,tests}.rs`, `ui/engine_panel.rs`, the mock fixture and this document. Modified: `main.rs`, `app.rs`, `ui/mod.rs`, `ui/game_review.rs`, README. No commit/push or unrelated-page redesign.

## Manual acceptance

Run from a graphical terminal when ready:

```sh
cd '/home/jairus/Antonius Jairus/Projects/Mfalmion'
env -u WAYLAND_DISPLAY MFALMION_WINDOW_SYSTEM=x11 VK_DRIVER_FILES=/usr/share/vulkan/icd.d/intel_icd.json WGPU_BACKEND=vulkan cargo run -p mfalmion-app --offline
```

- [ ] Open Game Review: engine starts Offline with no evaluation. Start Analysis; verify identity, Starting/Analyzing/Ready, score, depth, nodes, bestmove and SAN PVs.
- [ ] Play manually and import PGN/FEN; navigate by keys/buttons/history, create a local variation, switch games, Undo and New Game. Old evaluations must clear immediately and only the displayed position's results may appear.
- [ ] Navigate rapidly during a depth-16 MultiPV search. Verify responsiveness, no old PV flashes, and one reused engine process.
- [ ] Stop while analyzing and during startup; verify completion/Ready or Offline, then Start again. The engine must never play a move automatically.
- [ ] Test MultiPV 1/2/3, depth changes and Apply settings. Verify per-line depth consistency, readable bound/mate labels, and White-positive signs for positions with either side to move.
- [ ] Flip/resize while moving and analyzing. Confirm piece mappings/animations remain correct and the White-left/Black-right bar stays clearly labeled. Check the narrow/short-window scroll behavior.
- [ ] Set a missing/invalid engine path and Start; verify a readable Error and intact game. Restore `/usr/games/stockfish`, then retry successfully. Check thread/hash limits.
- [ ] Close while searching, while starting and after Stop. The window should remain responsive during short teardown and then close; `pgrep -a -x stockfish` should show no owned leftover process.

Stop after Milestone 03. Wait for explicit user approval before Milestone 04.
