# Milestone 06.2 — Opening classification evidence audit

## Finding and scope

No sign-conversion, move-index, arithmetic or threshold-implementation defect was found. `1. b3 — Inaccuracy` is reproducible from the saved Stockfish evidence and Mfalmion's documented v1 rule. A fresh depth-20 comparison still falls in that category. This establishes consistency with the current model, not that “Inaccuracy” is a universal or official chess judgment. The expected-score model and category boundaries are heuristic and have not been independently calibrated.

There are **78 matching games**, not one uniquely identifiable game. All 78 saved `b3` first moves are Inaccuracy. All 151 games and completed analyses were inspected read-only for their first move. No saved analysis, rule, threshold, accuracy formula, schema or engine configuration was changed. No opening-book exemption or move-specific exception was introduced. No GUI, Qwen, account reanalysis, commit/push or Milestone 07 work occurred.

## Located games and representative saved evidence

Representative game **ID 4 / run 4**: **Antonius-Jairus-R vs umbragetaken**, **2026.11.08**, result **1-0**. The full saved before/after MultiPV groups and move record are in [saved-b3 evidence](fixtures/milestone-06-2-saved-b3.json).

The most recent matching game is **ID 151**, Antonius-Jairus-R vs m7ammed2012, 2026.11.16: before **+0.38**, after **−0.13**, loss **0.06360189920516168**, also Inaccuracy. Every matching ID, and all other first moves, are listed in the [151-row audit CSV](fixtures/milestone-06-2-first-moves.csv).

| Item | Game 4 persisted value |
| --- | --- |
| Algorithm | `mfalmion-expected-score-v1` |
| Engine | Stockfish 17.1, `/usr/games/stockfish` |
| Settings | Depth 12, MultiPV 3, Threads 2, Hash 64 MiB |
| Move record | `ply=1`, White, SAN `b3`, UCI `b2b3`, Opening |
| Before rank one | White **+45 cp**, exact, best `e2e4` |
| After rank one | White **−16 cp**, exact, Black best reply `e7e5` |
| Before other ranks | `d2d4`: +24 cp; `g1f3`: +23 cp |
| After other ranks | `g8f6`: −8 cp; `e7e6`: −3 cp, all White perspective |
| Centipawn loss | 61 cp |
| Expected-score loss | **0.07600323070020171** |
| Applied category | Inaccuracy: **0.06 ≤ loss < 0.12** |
| Confidence | `configured depth`, not deeper-verified |
| Tags | Empty; no instability flag was stored |

Best alternative PV before `b3`:

```text
e2e4 e7e5 g1f3 g8f6 f3e5 f6e4 d1f3 e4g5 f3e3 g5e6 d2d4 d7d5 b1c3
```

Best reply PV after `b3`:

```text
e7e5 c1b2 b8c6 c2c4 g8f6 e2e3 d7d5 c4d5 f6d5 g1f3 c8f5 a2a3 e5e4 f3d4 c6d4 b2d4 d8d7
```

All 78 `b3` records have exact before/after rank-one bounds, the same engine settings and `e2e4` as the saved best alternative. Their before values range **+28 to +54 cp**, after values **−47 to −12 cp**, and losses **0.0623531197624993 to 0.11197179733903434** (mean **0.0848834304976635**). These are ranges across independent searches, not an interval for one engine result.

## Perspective, indexing and mathematics

UCI centipawn output is converted from the side-to-move perspective into White's perspective once in `engine/protocol.rs`. After `b3`, Black is to move, so raw Black-positive `score cp 16` converts to stored White `Cp(-16)`. It must not be flipped again when computing White's move loss. The original live UCI transcript was not stored, so the historical raw `+16` message is inferred from the adapter and saved normalized score; the conversion is directly regression-tested.

For centipawns `c`:

```text
Ewhite(c) = 0.5 + 0.5*tanh(c/400)
          = 1 / (1 + exp(-c/200))
Eblack(c) = 1 - Ewhite(c)
loss = max(0, Ebefore(mover) - Eafter(mover))
```

For game 4:

```text
Ewhite(+45) = 0.5560138905446199
Ewhite(-16) = 0.4800106598444182
loss        = 0.07600323070020171
```

This is about 7.6003 percentage points on the **heuristic quality scale**, not a calibrated probability of winning. Near equality the slope is 1/800 per centipawn, so a 0.06 cutoff corresponds to roughly a 48-cp deterioration: a strict model boundary. The independent logistic identity, fixed numeric values and White/Black complement are regression-tested.

The move's source is timeline index 0→1. Stored position 0 is the standard initial White-to-move FEN. Position 1 is:

```text
rnbqkbnr/pppppppp/8/8/8/1P6/P1PPPPPP/RNBQKBNR b KQkq - 0 1
```

`game_moves.ply=1`, `batch_moves.ply=1`, the summary's first move, SAN `b3`, UCI `b2b3`, and the White mover agree. The replay UI uses `cursor-1` to select the saved move, with a separate initial-position state. The all-first-move audit found **zero SAN/ply/side mapping discrepancies and zero loss calculation discrepancies**. A Rust fixture test recomputes every one of the 151 stored first-move labels from the saved scores and best move and matches every label.

## Deeper temporary Stockfish comparison

One Stockfish process performed **10 sequential depth-20 searches**, covering the standard initial position and all nine distinct first moves in this account. Settings remained **MultiPV 3, Threads 2, Hash 64 MiB**, with the same Stockfish 17.1 executable. The existing production adapter supplied the complete initial FEN plus move history, reset the hash/new-game search state, checked engine identity and legal PVs, and required complete depth-20 rank groups and `bestmove`. The existing 30-second per-search cap remained; all requests reached depth 20 with exact bounds. An exact UCI bound does not mean the finite-depth evaluation is the true game-theoretic value. Total explicit diagnostic test runtime: **33.19 seconds**.

No database was opened by this diagnostic. Results were written only to a new temporary JSON file and copied into the [depth-20 evidence artifact](fixtures/milestone-06-2-depth20.json). The artifact contains all three lines/PVs, actual settings, history commands and elapsed times. No saved record was replaced.

Initial-position depth-20 alternatives: **e4 +37 cp**, **d4 +22 cp**, **Nf3 +19 cp**, all White perspective. After `b3`: **e5 −21 cp**, **c5 −2 cp**, **Nf6 −2 cp**, all White perspective. Rank one after `b3` is best for Black, so its lower White score is expected.

| First move | Depth-20 after (White cp) | Loss against initial +37 cp | v1 category from deeper evidence |
| --- | ---: | ---: | --- |
| b3 | −21 | 0.072344450417848 | Inaccuracy |
| e4 | +27 | 0.012419705612879 | Best (matches rank-one root move) |
| d4 | +22 | 0.018646236701186 | Excellent |
| Nf3 | +26 | 0.013664234658461 | Excellent |
| c4 | +27 | 0.012419705612879 | Excellent |
| b4 | −36 | 0.090997433419360 | Inaccuracy |
| f4 | −24 | 0.076082592810351 | Inaccuracy |
| e3 | +8 | 0.036119874165814 | Good |
| g3 | +19 | 0.022386386919219 | Good |

Depth-20 `b3` best reply PV:

```text
e7e5 c1b2 b8c6 c2c4 g8e7 g2g3 d7d5 c4d5 e7d5 f1g2 f8c5 d1c1 c5b6 b1c3 c8e6 g1f3 d5c3 c1c3
```

This is a deeper comparison, not proof of the true game-theoretic score. Fresh multithreaded searches can vary. It supports the conclusion that the reported `b3` category was **not solely a depth-12 artifact in this comparison**: both saved and deeper evidence exceed 0.06. Numeric instability is still visible across repeated identical opening positions.

The current instability guard rejects large negative losses (apparent improvements over 0.03), and large absolute before/after discrepancies for a played best move. It cannot identify every positive search fluctuation on a non-best move. For `b3`, the played move is not the recorded best and the loss is positive, so v1 intentionally applies the ordinary threshold. Its lack of an instability tag is not a claim of high-depth certainty.

## Other first moves

All 151 games share the standard initial FEN, and the 78 `b3` games share the same after-move FEN/history prefix.

| Move | Games | Saved depth-12 categories | Saved loss range |
| --- | ---: | --- | --- |
| b3 | 78 | 78 Inaccuracy | 0.062353–0.111972 |
| e4 | 43 | 42 Best, 1 Excellent | 0–0.022262 |
| d4 | 20 | 12 Excellent, 8 Good | 0.004966–0.044303 |
| f4 | 3 | 3 Inaccuracy | 0.082240–0.098375 |
| Nf3 | 2 | 2 Good | 0.021180–0.023580 |
| c4 | 2 | 2 Good | 0.038609–0.044781 |
| b4 | 1 | 1 Inaccuracy | 0.076083 |
| e3 | 1 | 1 Good | 0.029892 |
| g3 | 1 | 1 Good | 0.038609 |

The deeper sample changes Nf3 and c4 from Good to Excellent and places d4 in Excellent. The three opening families labeled Inaccuracy (b3/b4/f4) remain Inaccuracy in the deeper sample. Thus shallow-search/category instability exists elsewhere, but there is no systematic sign or index bug in these opening records. A label near a boundary should not be treated as a timeless assessment.

## Threshold decision and recomputation implications

**No threshold adjustment was applied or recommended as calibrated.** This account is a narrow sample with 78 repetitions of the same first move, and the audit has no independent target-quality labels. Changing the cutoff until a specific opening gets the desired name would not be calibration.

A sensitivity check illustrates why a simple change is insufficient: moving the Good/Inaccuracy boundary from **0.06 to 0.08** would make the deeper b3 loss Good, but only **28 of the 78 saved b3 results** would move below it; **50 would remain Inaccuracy**. It would also change the saved b4 example to Good while its deeper result remains Inaccuracy. A 0.10 boundary would move 72 of 78 saved b3 cases below it. These counts are regression-tested; neither candidate is presented as an empirically validated replacement.

If a later calibration task adopts a different boundary, it must use a new classification-algorithm version and explicitly rebuild per-move labels, classification counts, phase counts and compatible aggregate views into new records while preserving v1. Existing saved classifications do not change merely because source thresholds change. A labels-only reclassification could reuse the validated losses/evidence under an explicitly defined compatibility policy; its accuracy values would remain unchanged if the formula and eligible moves are unchanged. Higher-depth reassessment instead needs explicitly requested new engine searches. Neither operation was run on the 151 saved analyses.

A sensible future calibration would use diverse independent positions, deeper repeated verification around decision boundaries and independently reviewed severity targets. Targeted verification/uncertainty near cutoffs is more defensible than blanket opening exemptions. No `b3` exception is needed or implemented.

## Tests, resources and data integrity

Added six deterministic regressions:

1. Exact game-4 b3 evidence, PV legality, indexing, 61-cp loss, normalized loss and Inaccuracy threshold.
2. Black UCI score converted once to White perspective.
3. Independent logistic equivalence and fixed expected-score numeric oracles.
4. Non-best opening receives an ordinary loss-based label; lower-loss b3 can be Excellent without an exemption.
5. Explicit limits of the existing instability guard.
6. All 151 saved first-move labels and normalized losses recompute identically; threshold sensitivity counts match.

Added one separate ignored opt-in diagnostic using the existing engine adapter. To reproduce, choose a **new** output path; existing files are refused:

```sh
MFALMION_OPENING_AUDIT_OUTPUT=/tmp/mfalmion-opening-audit-new.json cargo test --workspace analysis::opening_audit_tests::deeper_stockfish_opening_audit -- --ignored --exact --nocapture
```

Final required checks:

| Check | Result | Exit |
| --- | --- | --- |
| `cargo fmt --all -- --check` | Pass | 0 |
| `cargo check --workspace` | Pass | 0 |
| `cargo test --workspace` | 255 passed, 0 failed, 4 ignored | 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | Pass, no warnings | 0 |
| `git diff --check` | Pass | 0 |
| Explicit depth-20 diagnostic above | 1 passed; 10 complete depth-20 positions | 0 |

One preceding full-suite run exited 101 because the existing `engine::tests::cancellation_timeout_forces_reap_and_next_request_can_recover` fixture failed to execute with `Text file busy (os error 26)`. That test passed in isolation (exit 0), and the final complete suite passed 255/255 non-ignored tests (exit 0). No unrelated engine/fixture code was changed for this transient failure.

The normal suite's four ignored tests are the existing two real-engine smokes, live Chess.com smoke and new deeper diagnostic. Only the new deeper diagnostic was explicitly run for this milestone.

The real database was opened via SQLite `mode=ro` plus `PRAGMA query_only=ON`, never through the application writer. It still contains **151 games and 151 completed runs**. Its 14,245,888-byte main file SHA-256 before and after the engine comparison is unchanged:

```text
734888defe262a30e129db7caa43c9183f20bb59d7e76055da4d4965a42763f0
```

The WAL was empty at inspection. No database backup/restore, cleanup, package install or graphics/session change occurred. Initial SSD available space was **7,609,724,928 bytes**; the post-build reading was **7,544,958,976 bytes** (about 7.03 GiB). Diagnostic engine use was bounded to one process, two threads, Hash 64 MiB, CPU only; no pre-existing Stockfish process was running at its start.

## Changes and remaining limitations

Production classification, accuracy, engine and UI behavior are unchanged. `analysis/mod.rs` only registers a `#[cfg(test)]` module. Added:

- `crates/mfalmion-app/src/analysis/opening_audit_tests.rs`
- `docs/milestone-06-2.md`
- `docs/fixtures/milestone-06-2-saved-b3.json`
- `docs/fixtures/milestone-06-2-first-moves.csv`
- `docs/fixtures/milestone-06-2-depth20.json`

The task stops with evidence and regressions, not a speculative production fix. Depth 20 is finite; no broad calibration, official classification comparison or deep reanalysis of complete games was performed. Historical raw UCI transcripts are unavailable. The account's repeated identical openings are not independent calibration examples. No claim is made that these category names are official Chess.com judgments.

No commit or push commands were issued by this audit. During the task, the shared repository advanced externally to `e9fab6f` (`Move feedback`), which includes the audit tests and evidence artifacts. That history was left intact; final reporting edits in this document remain uncommitted. Existing work was preserved. Stop at Milestone 06.2; Milestone 07 remains unstarted.
