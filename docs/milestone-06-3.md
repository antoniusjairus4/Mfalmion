# Milestone 06.3 — Classification calibration research

## Decision

**V2 is not ready for production release.** This milestone implements the permitted fallback: an offline evaluation harness, version-controlled legal fixtures and recorded engine experiments, and an explicitly provisional policy. No production threshold, accuracy calculation, UI behavior, saved review, database schema or migration changed. No reclassification action is exposed. Milestone 07 has not begun.

The reviewed v1 normalization, mover perspective, move indexing, persistence version filters and Game Review mapping have no newly confirmed defect. Evidence of search sensitivity is not evidence of an implementation bug. A small authored corpus with no independent human reference labels cannot establish better coaching thresholds. The existing repeated Larsen openings are not independent calibration examples.

## Dataset and provenance

`docs/fixtures/milestone-06-3-cases.json` contains **32 fixtures**, all validated by the existing PGN parser and shakmaty. The target is the final played move; earlier moves establish the complete legal history. Custom positions use standard SetUp/FEN headers. Every fixture records its origin, intended interpretation, `reference_label: null`, and `provisional: true`.

These are original Mfalmion-authored test sequences and synthetic positions, not copied annotated games. Opening names and familiar tactical motifs supply context, not authoritative judgments. No external annotated corpus, proprietary labels or opening database is claimed.

Coverage:

- Fourteen opening fixtures: e4, d4, c4, Nf3, b3, f4, b4, a3, h4, g3, Sicilian, French, Caro-Kann and Scandinavian.
- Eleven tactical fixtures: hanging queen; candidate missed fork, pin and skewer; immediate mate and delayed mate; legal-mate sacrifice candidate and unsound offer; only legal defense; routine and forced recaptures.
- Seven positional fixtures: doubled-pawn tradeoff, king exposure, early queen development, space choice, quiet defense, pawn-ending tempo and winning-simplification candidate.

Interpretations are hypotheses to investigate. A pin is not automatically a winning tactic; doubled pawns are not automatically a mistake; an offered piece is not automatically a sound sacrifice. Deterministic tests additionally verify the two only-legal-move properties and actual checkmate. The synthetic sacrifice tests inherited from Milestone 06 verify evidence gates, not expert-approved Brilliant labels.

## Offline architecture and reproducibility

`analysis/calibration.rs` is registered only under `#[cfg(test)]`. It reuses `notation::GameRecord`, existing classification/metrics/phase functions, the existing history-aware `UciAnalyzer`, exact-group validation and the existing engine lifecycle. It contains no SQLite connection and no UI registration. Normal application builds do not contain the candidate policy or experiment runner. No dependencies were added.

The opt-in experiment uses Stockfish **17.1**, MultiPV **3**, Threads **1**, Hash **64 MiB**, at depths **12/16/20**, with the existing 30-second per-search bound. One process is reused; the existing adapter clears hash/new-game state and sends the complete initial FEN plus legal history prefix for every request. Same-depth complete rank groups, legal PVs, exact bounds and compatible configurations are required. Terminal snapshots come from authoritative rules rather than fabricated engine searches. Engine identity is checked by the existing adapter.

Ten selected fixtures yield **30 before/after pairs**: e4, d4, b3, f4, hanging queen, immediate mate, delayed mate, only defense, quiet middlegame defense and pawn-ending tempo. This is 60 evaluation calls, including three terminal after-position shortcuts (57 UCI searches). The first nine fixtures took 51.19 seconds; the additional middlegame fixture took 18.60 seconds. These are two explicitly invoked runs, about 70 seconds combined, not a runtime guarantee. No account game was reanalyzed. There was no existing Stockfish process at the initial diagnostic check.

`milestone-06-3-depths.jsonl` records full before/after evaluations, all PVs, exact engine configuration, depth and pair elapsed time. `milestone-06-3-report.json` records per-depth v1/proposed assessments, losses, best alternatives, phases, confidence, transitions, frequencies and sensitivity. The files total approximately 210 KB; no unbounded logging is introduced.

Normal offline reproduction, no engine or database:

```sh
cargo test --workspace analysis::calibration
```

Write a new report using the recorded evidence (refuses an existing output):

```sh
MFALMION_CALIBRATION_REPORT=/tmp/mfalmion-calibration-report-new.json cargo test --workspace analysis::calibration::recorded_evidence_and_offline_report -- --exact --nocapture
```

Explicitly rerun only the selected engine experiment, with a new output path:

```sh
MFALMION_CALIBRATION_OUTPUT=/tmp/mfalmion-calibration-search-new.jsonl cargo test --workspace analysis::calibration::stockfish_depth_experiment -- --ignored --exact --nocapture
```

Optionally set `MFALMION_CALIBRATION_CASE=b3` (or another selected ID) to restrict this run. Unknown/unselected IDs fail. Never point outputs at the real database. Fresh runs are not required to match numeric snapshots bit-for-bit across different engines, CPUs or configurations. Deterministic tests consume the recorded evidence instead of starting Stockfish.

## Results across depths

Adjacent-depth agreement is **17/20 (85%)**: 8/10 for depth 12→16 and 9/10 for depth 16→20. Eight of ten fixtures keep the same primary label across all three depths. These are consistency measurements, not accuracy against a human ground truth.

| Fixture | Depth 12 | Depth 16 | Depth 20 |
| --- | --- | --- | --- |
| e4 | Best | Best | Best |
| d4 | Good | Excellent | Excellent |
| b3 | Inaccuracy | Inaccuracy | Inaccuracy |
| f4 | Inaccuracy | Inaccuracy | Inaccuracy |
| Hanging queen | Blunder | Blunder | Blunder |
| Immediate mate | Best | Best | Best |
| Delayed mate | Excellent | Excellent | Excellent |
| Only defense | Best | Best | Best |
| Quiet defense | Good | Excellent | Best |
| Endgame tempo | Excellent | Excellent | Excellent |

Per-category unchanged/total adjacent transitions: Best 6/6, Blunder 2/2, Excellent 5/6, Good 0/2, Inaccuracy 4/4. These tiny denominators and correlated depths must not be interpreted as category reliability estimates.

Per-depth frequencies:

| Depth | Best | Excellent | Good | Inaccuracy | Blunder |
| --- | --- | --- | --- | --- | --- |
| 12 | 3 | 2 | 2 | 2 | 1 |
| 16 | 3 | 4 | 0 | 2 | 1 |
| 20 | 4 | 3 | 0 | 2 | 1 |

Brilliant, Great, Mistake and Miss occur zero times in this selected experiment. Zero advanced candidates does not establish a zero false-positive rate. The report explicitly records unknown false-positive counts as null. Human-reference confusion matrices are null because there are no valid reference labels; depth-transition counts are reported separately.

Phase coverage at each depth is five opening, one middlegame and four endgame fixtures, based on the existing material/development detector. The middlegame sample changes categories twice; opening d4 changes once; the four endgame labels remain stable. This coverage is much too small for phase-specific calibration. No phase or opening-popularity discount is applied.

The delayed-mate fixture does not lose its forced win: both before and after scores are winning mates. The bounded model therefore gives zero outcome loss, despite passing up the immediate mate. It is not automatically a Miss. The current model intentionally does not penalize mate distance; this is a documented limitation, not silently treated as centipawn loss.

## Larsen's Opening evidence

All scores below are White-positive centipawns. The best root alternative is e2e4 at each depth; all groups are exact and complete.

| Controlled one-thread run | Before b3 | After b3 | Expected-score loss | v1 label |
| --- | --- | --- | --- | --- |
| Depth 12 | +44 | −33 | 0.0959359033 | Inaccuracy |
| Depth 16 | +27 | −31 | 0.0723714406 | Inaccuracy |
| Depth 20 | +37 | −21 | 0.0723444504 | Inaccuracy |

The formula remains `Ewhite(c)=0.5+0.5*tanh(c/400)`, `Eblack=1−Ewhite`, and loss is consistently mover-perspective before minus after, with existing instability exclusions and nonnegative clamping. This is a bounded quality model, not a calibrated probability of a human result. The 0.06 Inaccuracy boundary is crossed at all depths.

Milestone 06.2's saved two-thread representative was +45→−16 at depth 12 (loss 0.0760032307), and its two-thread depth-20 comparison was +37→−21 (loss 0.0723444504). The new depth-12 result is a different controlled configuration, not a replacement of saved evidence. Do not attribute the difference purely to depth. Within the new controlled run, the loss spread across depths is about 0.02359, so candidate confidence is **Provisional**, despite category agreement.

There is a persistent finite-search evaluation concession relative to the engine's preference. No decisive material-winning or mating continuation is identified by the existing bounded tactical-PV detector for this opening comparison. That supports an engine-quality warning, not a claim that b3 is a tactical blunder. It does not establish the correct coaching severity or prove the position objectively inferior at arbitrary depth. Opening familiarity supplies no exemption and b3 has no special case in code.

## Proposed policy and threshold justification

Candidate identifier: **`mfalmion-expected-score-v2-proposal-1`**. Production remains **`mfalmion-expected-score-v1`**. The candidate intentionally makes **zero primary-label changes across the 30 recorded pairs**. It is an evidence/presentation research policy, not a released recalibration.

Retained rules:

1. Missing/invalid/incomplete/incompatible groups, non-exact bounds, terminal roots and existing search-instability exclusions → Insufficient evidence, excluded loss.
2. A valid played root-best move → Best; otherwise concrete tactical evidence may qualify for Miss under existing thresholds; otherwise use the existing ordinary bands.
3. Excellent `<0.02`; Good `[0.02,0.06)`; Inaccuracy `[0.06,0.12)`; Mistake `[0.12,0.25)`; Blunder `≥0.25`. Absolute evaluation enters through the nonlinear bounded transformation: equal centipawn changes in balanced and already-decided positions need not have equal loss. Mate winners and draws remain distinct typed values.
4. Miss retains precedence over ordinary Mistake/Blunder only with concrete legal PV tactical evidence and existing before≥0.75, after≤0.55, loss≥0.20 requirements. Non-best alone cannot create Miss. Every move has one primary category.
5. Phase, forced status, checks/mates and material context remain available evidence/tags. No new unsupported phase cutoff, popularity heuristic or practical-character score is invented.

Raising the Good/Inaccuracy boundary to 0.08 would change zero depth-12 labels, two depth-16 labels and two depth-20 labels in this selected corpus. At 0.10 it changes two at each depth. These are b3/f4 changes, not proof of better judgment; the hanging-queen Blunder remains. The report also counts all losses below each hypothetical boundary, including Best, separately from actual primary changes. The previous 78 saved b3 examples are highly correlated and cannot justify optimizing those boundaries. **Retain 0.06 until independent diverse annotation and held-out testing support a change.** Accuracy remains the v1 side-separated mean of `100*exp(-5*loss)`; no accuracy recomputation occurs.

### Engine quality versus coaching significance

The assessment preserves objective loss, best move, PV, phase, depth and engine category. A separate coaching field says either Insufficient evidence, concrete PV tactical evidence with uncalibrated practical severity, engine concession with practical significance unvalidated, or no measured loss without an exceptional-move claim. It cannot override objective data and is not a generated explanation. There is no Qwen integration or claim to Chess.com's proprietary labels.

### Confidence

The candidate uses discrete evidence states, never percentages:

- **Insufficient:** validation/configuration/complete exact-group failure, or v1 cannot produce an eligible loss.
- **Provisional:** one distinct depth; disagreement in primary category or best root; or loss spread ≥0.02 across depths.
- **Moderate:** at least two distinct compatible exact depths agree in category/root and loss spread is <0.02.
- **High:** all of 12/16/20 satisfy those agreement checks.

The 0.02 stability band reuses the existing negligible-loss band as an explicitly provisional research criterion. It is not independently calibrated. High means consistency of this bounded engine evidence, not human-label correctness or exceptional-move proof. All policy outputs additionally carry `policy_provisional=true`; no state releases the proposal. Seven selected fixtures meet High evidence consistency; b3, d4 and quiet defense remain Provisional.

### Brilliant and Great

Both labels remain in the shared category model and production v1 evidence tests. They are deliberately not emitted by the offline proposal: this dataset has no independently annotated exceptional examples capable of validating their release.

A future release must additionally require stable deeper root/after evidence; a consequential decision and significant exact alternatives gap; no routine only move or recapture; and, for Brilliant, a legally demonstrated meaningful sacrifice/exceptional feature, sound continuation and no refutation at the declared verification budget. The current narrow sacrifice detector checks accepted offers and a persistent material deficit over four plies; it is not sufficient proof of exceptional human significance. Existing v1 gap/verification gates must not be weakened merely to populate statistics. Until these conditions are validated, fall back to an ordinary label with provisional evidence, rather than promising Brilliant/Great from Stockfish agreement alone.

## Persistence, preview and compatibility

There is **no migration and no database reclassification workflow**, as explicitly permitted when validation is insufficient. The offline report is an immutable preview of candidate versus v1 on fixtures only; it does not preview/rewrite the 151 real reviews. The real database was never opened through the application or test storage writer in this milestone.

Production config validation rejects the proposal algorithm; versioned configuration keys differ. A new temporary-database regression verifies that attempting to create a proposal job performs zero writes, preserves the completed v1 summary, cannot load it as a proposal result, and leaves Game Review selecting v1. Existing tests cover version isolation, transactional rollback and read-only review loading. Offline v1 and proposal assessments coexist in separate report fields. There is nothing to roll back in the real database, and no UI toggle falsely implying v2 is ready.

At the initial and final checks, the real 14,245,888-byte database has the same SHA-256:

```text
734888defe262a30e129db7caa43c9183f20bb59d7e76055da4d4965a42763f0
```

Its WAL is empty. Existing 151 completed analyses, accuracy values and engine evidence remain untouched. The fixed review toolbar, independent scrolling, board and live engine settings have no code changes.

## Verification

| Check | Actual result | Exit |
| --- | --- | --- |
| `cargo fmt --all -- --check` | Pass | 0 |
| `cargo check --workspace` | Pass | 0 |
| `cargo test --workspace` | 267 passed, 0 failed, 5 ignored; 272 discovered | 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | Pass, no warnings | 0 |
| `git diff --check` | Pass | 0 |
| Explicit selected Stockfish experiment | 1 passed, nine fixtures × three depths | 0 |
| Explicit additional quiet-defense experiment | 1 passed, one fixture × three depths | 0 |

Twelve new deterministic passing tests cover fixture legality/provenance; saved b3 and mate evidence; complete/distinct-depth confidence; instability; boundaries and loss preservation; phase context; forced/sacrifice conservatism; pure preview; production version rejection; zero-write persistence and Game Review compatibility. Existing tests retain perspective, common openings, sound/unsound sacrifice, Brilliant/Great evidence gates, Miss evidence/precedence, mate handling, rollback and CPU-only layout coverage. No new native GUI or graphical automation was run. Five normally ignored tests are the two pre-existing real-engine smokes, live Chess.com smoke, 06.2 diagnostic and this new calibration experiment.

## Resources, files and Git

Before compilation available SSD space was **7,543,746,560 bytes** (about 7.03 GiB). After verification it was **7,535,587,328 bytes** (about 7.02 GiB); no cleanup, package installation or release rebuild was performed. Qwen, the desktop/session and external archives were not touched. No GUI, full-account engine analysis, commit or push was started.

Created:

- `crates/mfalmion-app/src/analysis/calibration.rs`
- `docs/fixtures/milestone-06-3-cases.json`
- `docs/fixtures/milestone-06-3-depths.jsonl`
- `docs/fixtures/milestone-06-3-report.json`
- `docs/milestone-06-3.md`

Modified by this milestone:

- `crates/mfalmion-app/src/analysis/mod.rs` — test-only module registration.
- `crates/mfalmion-app/src/analysis/tests.rs` — temporary-database protection regression.

Pre-existing uncommitted `docs/milestone-06-2.md` changes were preserved. Final Git status contains these three modified tracked files and the five new files above, all uncommitted.

## Acceptance and remaining limitations

- Inspect the 32 legal fixtures and their provisional interpretations; obtain independent expert annotations, particularly exceptional moves and missed opportunities, before tuning thresholds.
- Inspect recorded before/after PVs, the b3 table, phase/category counts and stability report. Confirm that the depth-transition table is not presented as a human confusion matrix.
- Reproduce the offline tests/report without Stockfish or database access. Any fresh engine experiment requires the explicit ignored-test command and a new output file.
- If desired, manually open an existing v1 Game Review and confirm navigation, labels and fixed toolbar remain as previously accepted. This native visual check was not performed here; no v2 UI should appear.
- Do not reclassify existing reviews until a separately accepted policy and explicit transactional preview/rollback workflow are implemented.

Limitations: only ten of 32 fixtures received new engine searches; there is one middlegame example and sparse tactical diversity in the searched subset; there are no independent reference labels, held-out calibration set, repeat-run variance estimates, or validated practical-severity model. No false-Brilliant/Great rate or human classification accuracy can be estimated. Finite-depth stability does not prove chess truth. The current four-ply sacrifice heuristic and bounded tactical PV detector have limited coverage. Claimable draws/custom-start history limitations and mate-distance treatment remain those of v1. These gaps justify retaining production v1, not silently rewriting the user's reviews.

**Stop after Milestone 06.3. V2 remains a proposal; Milestone 07 requires explicit approval.**
