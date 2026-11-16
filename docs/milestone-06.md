# Milestone 06 — Resumable account analysis

Implemented for manual acceptance. Milestone 05 acceptance and the 149 synchronized games are prerequisites supplied by the user. The GUI and real account database were not opened during development. No account batch was started, no Qwen process started, and no commit or push performed. No Milestone 07 work is included.

## Architecture and ownership

`analysis/` contains the immutable analysis model, normalized metrics, classification rules, feature-based phase detector, sequential game runner and bounded background scheduler. It uses the existing `notation::GameRecord`, shakmaty 0.30.1 validation and indexed `Chess` snapshots, existing `EngineClient`/UCI transport, existing SQLite `Store`, and existing persistent `jobs` lifecycle. There is no second parser, engine implementation, database, or board.

The Game Library's Account Analysis controls explicitly create work. Startup, opening a game and Chess.com sync never start analysis. Default batch settings are depth 12, MultiPV 3, Threads 2, Hash 64 MiB; separate `batch_analysis` settings retain the engine executable, exact identity and algorithm version. Interactive settings remain separate. Resume uses the original job configuration, including its fresh-search/reuse policy, irrespective of subsequent preference changes.

The scheduler has a two-command bounded channel, one background SQLite connection and a coalesced shared progress snapshot. It selects one queued game at a time with SQL, loads only that game's validated timeline, and owns one existing UCI worker for the active job. It never collects all account games or evaluations in memory. Account statistics stream one completed summary at a time. Results for the selected review use an `Arc` so frame polling does not clone all move records.

The app requests interactive engine shutdown before dispatching a batch job and disables interactive Start while a job is queued/running. An engine-client lease in production prevents overlapping interactive/batch ownership even if a UI request races. The lease lasts through child reaping. Pause, cancel, completion and failure release the batch engine. Restarting interactive analysis recreates its stopped client. Test builds permit parallel isolated mock-engine tests; the real smoke runs separately.

Profile changes, profile/game deletion and restore are blocked while analysis is active. Pause and wait first. New sync operations are deferred while a batch owns resources. Existing profile foreign keys and scoped game loading remain authoritative. Use one Mfalmion instance at a time; startup crash recovery assumes another instance is not actively using the same database.

## Stockfish lifecycle and history

Every nonterminal batch request sends:

```text
ucinewgame
setoption name Clear Hash
isready
position fen <complete initial FEN> moves <all preceding standard UCI moves>
go depth <requested depth> movetime 30000
```

The existing worker performs UCI initialization, option validation and readiness barriers. Batch requires Clear Hash support. Before a cached nonterminal game can be resumed, a discarded depth-1 probe confirms the actual engine identity against the saved configuration. Every subsequent request also checks the identity. Unsupported engines or identity changes fail visibly.

The history is replayed legally with shakmaty and its complete final FEN must equal the requested indexed snapshot. Custom starts retain their initial FEN, side, rights, en passant and counters. Standard castling UCI identifiers and promotion suffixes come from shakmaty. History prefixes, not final FEN alone, identify repetition contexts. The old FEN-only interactive cache is never used as batch evidence. Custom initial positions explicitly disclose that pre-FEN history is unknown.

A result requires `bestmove` and a complete same-depth MultiPV group at the requested depth. Expected ranks equal `min(configured MultiPV, legal move count)`. Unique root moves and every PV move are validated legally. Search limits, transport failures or partial groups cannot become completed checkpoints. Scores and exact/lower/upper bounds remain separate. A complete bounded group can be persisted, but bounded before/after scores cannot produce a classification or accuracy contribution.

The existing 30-second search budget and watchdog are retained. Requested depths that cannot complete within that budget fail rather than masquerading as completed depth. One engine process is reused, but hash/history search state is reset for each position.

Checkmate, stalemate, insufficient material, automatic 75-move and fivefold draws use authoritative chess state/history and need no engine search. Mate takes precedence over automatic draws. Threefold/50-move availability is tagged as claimable, not silently claimed. Resignation and timeout results remain metadata: the final board is not fabricated into checkmate or a terminal engine score. Subsequent moves after an automatic terminal position, if present in an imported record, are excluded from classification.

## Evaluation model and accuracy

Algorithm identifier: `mfalmion-expected-score-v1`. All formulas and thresholds below belong to Mfalmion and do not reproduce Chess.com's proprietary classifications or accuracy.

For White-positive centipawns `c`, the bounded quality transformation is:

```text
Ewhite(c) = 0.5 + 0.5 * tanh(c / 400)
Eblack(c) = 1 - Ewhite(c)
```

A proven mate maps to 1 for the mating player and 0 for the opponent; a terminal draw maps to 0.5. Mate distances remain stored separately and are never converted into centipawns. The transformation is a quality model, not a calibrated win probability.

Each move uses the best root evaluation before the move and the evaluation after the played move, consistently from its mover's perspective:

```text
signed_loss = E(before, mover) - E(after, mover)
loss = clamp(signed_loss, 0, 1)
move_accuracy = 100 * exp(-5 * loss)
side_accuracy = arithmetic mean(move_accuracy for that side's eligible moves)
```

An improvement greater than 0.03 is search instability and remains unclassified. A played best move with an absolute before/after discrepancy above 0.03 is also unclassified. Small negative differences clamp to zero with a search-improvement tag. Centipawn loss is a nonnegative 64-bit difference only when both scores are centipawns. Bounds, missing evidence and terminal roots are excluded. Zero eligible moves produce an absent accuracy, displayed as a dash. Excluded counts and separate White/Black eligible counts accompany every result, so low coverage is visible. No opening-book database exists and no opening moves are exempted.

Derived calculations are deterministic for identical saved engine evidence and algorithm version. **Fresh Stockfish searches with Threads 2 are not guaranteed bit-for-bit reproducible**, even with the same version/settings/history. No such guarantee is claimed. Clear Hash removes prior-search hash contamination, exact compatible checkpoints pin completed evidence, and Threads 1 is available for stronger repeatability. A forced fresh reanalysis can legitimately differ. Portable floating-point aggregate validation tolerates JSON round-trip differences below 1e-9; integer counts and timeline identity remain exact. This is a limitation relative to an absolute fresh-search determinism requirement.

## Primary classifications and evidence

Precedence is Insufficient analysis, Brilliant, Great, Best, Miss, then the ordinary loss thresholds. Exactly one primary category exists per move; tags are non-exclusive.

| Category | Rule |
| --- | --- |
| Best | Played UCI move equals the completed root's rank-one PV move, after consistency gates. |
| Excellent | Non-best loss below 0.02. |
| Good | Loss from 0.02 to below 0.06. |
| Inaccuracy | Loss from 0.06 to below 0.12. |
| Mistake | Loss from 0.12 to below 0.25. |
| Blunder | Loss at least 0.25. |
| Miss | Concrete winning tactical evidence, before quality at least 0.75, after at most 0.55, loss at least 0.20. |
| Great | Verified best move, loss below 0.02, difficult/consequential position, exact alternative gap at least 0.15; not an only move or routine recapture. |
| Brilliant | Verified best move, loss below 0.02, at least three exact alternatives, gap at least 0.20, before quality at least 0.60, concrete sound consequential sacrifice; not an only move or routine recapture. |

Miss requires an engine PV proving mate for the mover, or a legal forcing PV (capture/check) with a net material gain of at least three pawns over at most twelve plies. A winning score alone is insufficient.

A Great candidate must preserve a material deficit of at least three pawns or avoid an alternative with mover quality below 0.35. Sacrifice evidence can also qualify as consequential for targeted verification. Routine immediate recaptures to the preceding capture square are excluded from Great/Brilliant.

Brilliant evidence is deliberately narrow: a quiet non-pawn, non-king piece offer; the opponent's best verified PV immediately captures that offered piece; the net material deficit persists through four legal plies; deeper exact scores still support the move and alternatives are substantially worse. Ordinary captures, unaccepted offers, immediate material recovery, forced moves and unsound offers cannot earn Brilliant. Exceptional tactics outside this conservative detector receive ordinary categories.

At most four candidate moves per game receive extra before/after searches at `max(base depth + 4, 16)`, when base depth is at most 28. Great/Brilliant require complete exact alternative evidence and verification depth at least 16. Otherwise they fall back to ordinary classification. At base depths 29–32 no higher-depth verification is attempted. Every move records the actual classification depth and confidence (`configured depth`, `verified`, or `insufficient`). No statistical quota creates advanced categories.

Tags include Check, Checkmate, Only move, Sacrifice, Tactical opportunity, claimable draw, custom-start history limitations and search instability. Phase is a separate persisted field.

## Phases and statistics foundation

Material values are pawn 1, knight/bishop 3, rook 5 and queen 9. Phase uses the position before each move, without a fixed move-number cutoff:

- Endgame: combined non-pawn material at most 20, or no queens and non-pawn material at most 30.
- Opening: otherwise non-pawn material at least 52 and at least four correctly colored knights/bishops still occupy their original home squares.
- Middlegame: all remaining positions.

Per-phase White/Black accuracy, eligible/excluded counts and classification counts are persisted in each completed summary. This feature heuristic can switch phase in unusual promotions/material configurations; it is an estimate, not an opening database.

`Store::account_statistics(profile, config)` streams only the latest completed compatible run per game. It exposes analyzed-game count, account-side game-average estimated accuracy, move-weighted sums/counts, classification totals/rates, phase sums/counts and White/Black sums/counts. Only a player name matching the selected profile username case-insensitively contributes to account performance; other manually imported games remain eligible stored games but do not pollute that player's performance. Coverage is available separately via `batch_coverage`. Algorithm, full configuration and engine identity are isolated. No full Dashboard is implemented.

## SQLite migration and persistence

Schema v3 is additive and transactional, preserving existing v1/v2 data, original PGNs, cache, synchronization records, settings and jobs:

| Table | Role |
| --- | --- |
| `batch_jobs` | Existing job FK, exact serialized configuration, stable configuration key and saved cache-reuse policy. |
| `batch_runs` | One game per job, configuration/algorithm, lifecycle state/error, final summary and timestamp. Previous complete runs survive fresh reanalysis. |
| `batch_positions` | Run/ply/depth checkpoint, SHA-256 full initial-position/UCI-prefix context, complete engine group, bounds/PVs and timestamp. |
| `batch_moves` | Per-run/ply derived loss, SAN, side, classification, tags, confidence, actual depth, best move/PV and phase as serialized records. |

Configuration hashes include engine path, exact identity/version, depth, MultiPV, Threads, Hash and algorithm version. The algorithm version namespaces the history-aware adapter, reset policy and fixed search budget. Checkpoints additionally require matching history digest, game, ply, depth and configuration, and are validated on retrieval. No lower-depth substitution, engine-version mixing or partial-rank reuse occurs.

Each complete position group is atomically checkpointed. Final move records and the completed summary/status are committed in one transaction, only after all base-position checkpoints and every move result are present and validated. The summary includes complete settings, accuracy, coverage, phase aggregates and one primary count per move. A crash cannot leave partial records falsely marked complete. A previous completed review remains readable until a new run finishes.

The existing WAL/FULL-synchronous connection configuration, foreign keys and bounded busy timeout remain. Migrations and restore validate the new tables; old backups are upgraded on restore. Normal application startup performs recovery through the existing storage worker. Tests use temporary databases, including mock UCI and the real shallow smoke.

## Jobs, pause, resume and recovery

Analyze All Unanalyzed selects stored games belonging to the active database profile that have no completed compatible review. Analyze Selected follows the same rule. Reanalyze Selected creates a fresh revision, ignoring other runs' checkpoints while retaining its own restart checkpoints. Retry Failed selects games whose latest finished compatible attempt failed; a later successful retry is not repeated. Retry count is persisted in the new job. All queue creation and settings capture are transactional.

The parent job uses the existing Pending/Running/Paused/Completed/Failed/Cancelled lifecycle. Per-game queue entries distinguish completed and failed work. One failed game is recorded visibly and the scheduler continues to the next. Parent completion counts processed games, including failures, and the parent becomes Failed if any game failed. Engine-creation/job-level failures mark remaining unfinished entries failed for explicit retry.

Pause/cancel interrupts an in-flight UCI request and discards its incomplete group. Completed groups remain valid. Pause retains unfinished work for explicit Resume. Cancel marks unfinished entries cancelled but retains completed reviews/checkpoints. Running jobs and batch jobs pending before their first search recover as paused; startup never starts Stockfish. The indexed position checkpoints, rather than a potentially stale in-memory cursor, determine durable progress. Progress UI is refreshed at position boundaries. If a queued game is explicitly deleted while paused, its queue records cascade and job totals are reconciled on resume rather than leaving an impossible completion count.

No ETA is displayed because a reliable observed-speed estimate has not been implemented. Progress reports games and current positions directly.

## UI and manual acceptance

Game Library adds selection per row, latest attempt status and Account Analysis controls: depth/resources, selected profile, coverage, current game/position, progress, Analyze All Unanalyzed, Analyze Selected, Reanalyze Selected, Pause, Resume, Cancel and Retry Failed. Save batch settings is independent of interactive settings. Coverage and summaries refer to the selected exact configuration; per-row latest-attempt labels may describe another configuration and are not used to decide compatibility.

Game Review shows **Mfalmion Estimated Accuracy**, both sides, eligible/excluded counts, all nine primary category totals, selected-move category/loss/confidence/depth/alternative/PV and a scrollable classification list. Existing move-history navigation selects the displayed result. Local variations and unsaved sessions do not inherit the stored mainline's classifications. Review identity includes the canonical game fingerprint, preventing ID reuse after a restore from attaching results to a different in-memory board. Board artwork, colors, legal rules and animation implementation are unchanged. PV text is currently UCI notation.

Launch manually under X11:

```sh
cd '/home/jairus/Antonius Jairus/Projects/Mfalmion'
env -u WAYLAND_DISPLAY MFALMION_WINDOW_SYSTEM=x11 VK_DRIVER_FILES=/usr/share/vulkan/icd.d/intel_icd.json WGPU_BACKEND=vulkan cargo run -p mfalmion-app --offline
```

Manual checklist (not performed):

1. Confirm startup shows the selected profile and existing library; no Stockfish batch starts.
2. Select one short game, choose shallow batch depth, start selected analysis, and verify responsive navigation/resizing and only one engine owner.
3. Pause mid-search, wait for paused status, inspect another page, resume and verify saved progress is reused.
4. Close while running; reopen and confirm paused recovery and explicit Resume requirement.
5. Cancel a job; verify completed game reviews remain, unfinished games stay unanalyzed, and a new job can start.
6. Test invalid engine path/identity, then correct it and Retry Failed. Confirm successful games are not repeated.
7. Open an analyzed game; replay both directions, flip the board, inspect losses/alternative/PV, and confirm side accuracy and selected classification track the timeline.
8. Create a local variation; verify mainline results disappear until the stored mainline is restored.
9. Change configuration and verify prior incompatible accuracy is hidden. Reanalyze, pause, restart and confirm its original fresh-search policy survives.
10. Check profile isolation, blocked destructive actions while running, safe profile switch after pause, and separate interactive settings.
11. Only after these checks, explicitly initiate the desired account batch. Observe CPU use and pause if the laptop becomes busy.
12. Verify castling, en passant, promotion chooser, captures and existing animations visually; automated rule/animation regressions remain covered without launching a native window.

## Performance, storage and limitations

No additional dependency or system package was introduced. Engine memory is bounded by configured Hash plus normal engine overhead; the default is two threads and one process. Batch commands/results are bounded, one game is materialized at a time, and the existing 4096-ply/import limits remain. There are no production analysis log files and no repeated PGN copies per evaluation.

Before writing a position checkpoint, database plus WAL/SHM size is checked against a 512 MiB safety limit. Exceeding it fails visibly and never deletes data. This is a conservative admission guard, not an exact SQLite page quota; the crossing group/final transaction may add a small amount beyond the threshold. All compatible revisions and valid partial work are retained. Repeated forced reanalyses therefore consume space until the guard stops further checkpoints. Future explicit retention/cache cleanup is deferred; no destructive cleanup runs automatically. Existing confirmed game/profile deletion still cascades only associated records.

Total runtime depends strongly on depth, position complexity, MultiPV, CPU load and verification candidates. Each game generally needs one base search per nonterminal indexed position plus a shallow identity probe; up to eight additional searches verify four candidates. Shallow smoke timing is not an account runtime prediction. No deep analysis of the actual 149-game library was performed.

Known limitations: native GUI acceptance remains manual; fresh multithreaded search is not absolutely deterministic; model thresholds/accuracy are heuristic and uncalibrated; Brilliant detection intentionally recognizes only a narrow sacrifice pattern; pre-custom-FEN history is unknowable; mate distances are stored but the accuracy model treats all proven mates for a side equally; bounded/unstable evaluations can yield low coverage; existing duplicate PGN limits still apply; one application instance is assumed; ETA and configurable retention cleanup are deferred. No Qwen coaching, full Dashboard, engine play, cloud or portable packaging was added.

## Verification and disk

Final verification on 2026-11-16:

| Check | Actual result | Exit status |
| --- | --- | --- |
| `cargo fmt --all -- --check` | Pass | 0 |
| `cargo check --workspace` | Pass | 0 |
| `cargo test --workspace` | 234 passed, 0 failed, 3 ignored (237 discovered) | 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | Pass, no warnings | 0 |
| `git diff --check` | Pass | 0 |
| `cargo test --workspace analysis::tests::real_stockfish_batch_smoke -- --ignored --exact --nocapture` | 1 passed, 0 failed; one temporary two-ply game, depth 2, MultiPV 1, Threads 1, Hash 16 MiB; Stockfish 17.1 | 0 |

The normal suite skips the opt-in real batch smoke, existing real interactive-engine smoke and live Chess.com API smoke. Only the new shallow real batch smoke was run explicitly. The final normal suite includes 64 additional passing Milestone 06 tests beyond the 170-test prerequisite baseline. CPU-only existing egui unit tests do not open windows or automate the desktop.

Free SSD space before building: **7,635,415,040 bytes** (7.11 GiB). Final measured available space after verification: **7,633,600,512 bytes** (7.11 GiB). No build cleanup or package installation was performed. Initial Git working tree was clean; final state is 16 modified tracked files and 10 new files, all unstaged, with no commit/push.

 Development checks found and fixed fractional JSON aggregate validation, reanalysis-resume cache policy, pending-before-first-search recovery, stale failed-retry selection, and queued-job ownership races. Tests cover metrics, all classification thresholds and evidence gates, forced moves, terminal/draw rules, phase statistics, full-history UCI, configuration/cache/profile isolation, checkpointing, transactions, persistence/reload, pause/resume/cancel/retry/recovery and real bounded worker communication. Existing board, PGN/FEN, sync, persistence and engine regressions remain included.

## Files

Created:

- `crates/mfalmion-app/src/analysis/{mod,classify,metrics,phase,runner,worker,tests}.rs`
- `crates/mfalmion-app/src/storage/batch.rs`
- `crates/mfalmion-app/src/ui/batch_panel.rs`
- `docs/milestone-06.md`

Modified:

- `README.md`
- `crates/mfalmion-app/src/{app,main}.rs`
- `crates/mfalmion-app/src/engine/{mod,worker,tests}.rs`
- `crates/mfalmion-app/src/storage/{mod,schema,games,jobs}.rs`
- `crates/mfalmion-app/src/ui/{mod,library,game_review,engine_panel}.rs`
- `crates/mfalmion-app/src/sync/tests.rs` (schema-version expectations only)
- `crates/mfalmion-app/tests/fixtures/mock_uci.py` (offline history-aware mode)

No Cargo dependency/lockfile changes, board asset changes, engine settings changes, real database writes, commits or pushes.
