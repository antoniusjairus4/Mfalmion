# Milestone 04 — Local SQLite persistence

Implemented 2026-11-16 after the user's Milestone 03 acceptance. This milestone adds local storage, a Game Library, completed-position cache, persistent settings and a resumable-job API. It does not add Chess.com requests, account batch analysis, accuracy/classification algorithms, Qwen, cloud services or Milestone 05. The accepted board/rules/artwork/animations/replay and UCI worker remain authoritative. No native GUI launch, graphical automation, commit or push was performed.

## Architecture and dependencies

`storage/` is separate from egui and chess-rule authority. `mod.rs` owns opening, migration, profiles, path resolution, integrity, size, backup and restore. `schema.rs` contains version 1 DDL. `games.rs`, `analysis.rs`, `jobs.rs` and `settings.rs` implement their respective operations. `worker.rs` owns one connection on one named thread, bounded command/result queues of four entries, thread-safe repaint notification and asynchronous shutdown. `tests.rs` uses only owned temporary directories/databases. The UI sends explicit operations, not per-frame queries; library pagination returns at most 50 rows.

New direct dependencies: rusqlite 0.43.0 with bundled SQLite and backup, serde/serde_json for validated settings/cache records, sha2 for SHA-256 fingerprints. Cargo.lock pins resolution. The bundled library reported SQLite **3.54.1**. This builds SQLite locally as a small dependency; no system package or separate engine binary was installed. The [rusqlite API](https://docs.rs/rusqlite/0.43.0/rusqlite/) provides parameter binding, transactions and the SQLite online backup interface. Existing shakmaty remains exactly 0.30.1.

SQL values use parameter binding through prepared statements (rusqlite prepare/query/execute). Static schema/PRAGMA statements contain no user input. The only constructed table-name SQL uses a fixed internal whitelist during restore validation/tests. Game inserts/deletes and profile/job changes involving multiple records use transactions. Database errors are returned as readable status text; no database is deleted on error.

## Schema and migration

| Entity | Content / ownership |
| --- | --- |
| profiles | Case-insensitive unique username, optional display name, active flag, timestamps, nullable last sync and sync status. Partial unique index permits at most one active profile. |
| documents | SHA-256-unique original PGN fragment/document, retained once and shared by referencing games. |
| games | Profile FK, optional external identifier, canonical PGN, original-document FK, players/date/time control/time class/result/ECO/opening, canonical fingerprint and import/update timestamps. Unique `(profile_id, fingerprint)`. |
| game_moves | `(game_id, ply)` primary key, complete FEN at every index including zero, SAN/UCI for subsequent positions, optional clock field. |
| analysis | Game/ply/history identity, engine name/version, serialized configuration fingerprint, depth and MultiPV, one JSON group of ranked cp/mate/bound/PV lines, completion status and timestamp. Composite uniqueness avoids redundant group snapshots. |
| jobs | Profile and optional game FK, kind/work identity, lifecycle status, total/completed units, last position, timestamps, last error and retries. Partial unique index prevents duplicate pending/running/paused work per profile/work key. |
| settings | Key, serialized validated value and update timestamp. |
| schema_migrations | Applied schema version and timestamp. |

Foreign keys are enabled. Deleting a selected game cascades its moves, analysis and jobs. Deleting a selected profile cascades only its data; unreferenced source documents are removed transactionally. Other profiles and their shared original documents remain. Deletion requires explicit UI confirmation and a checked `confirmed` API argument. The final profile cannot be deleted until another profile is created/selected, ensuring an active profile remains available. Profile switching keeps the review in memory; replacement guards still protect local work.

`PRAGMA user_version` and the migration ledger identify schema version 1. The initial schema and application ID `0x4d46414c` are created in one transaction. Existing current databases reopen without recreating tables. Future versions are rejected before journal-mode changes; unrelated nonempty databases are rejected without migration. Future migrations must be additive and transactional; none drop/rebuild existing user tables in this milestone.

## Data location and startup configuration

Precedence:

1. `MFALMION_DATABASE`: explicit database file.
2. `MFALMION_DATA_DIR`: directory containing `mfalmion.sqlite3`.
3. `$XDG_DATA_HOME/mfalmion/mfalmion.sqlite3`, otherwise `$HOME/.local/share/mfalmion/mfalmion.sqlite3`.

Only the default application directory is created automatically. Explicit override parents must already exist, so a missing data subdirectory/drive produces an error. Relative overrides resolve against the launch working directory. Paths inside any detected Git repository (including target/) are rejected; no developer-home path is compiled into the configuration policy. File paths are passed directly, with no shell interpolation. Environment paths support native OS strings; sidecar-size inspection preserves non-UTF-8 names.

Set `MFALMION_REQUIRE_DATABASE=1` when an existing configured file is expected. If that file is absent, startup fails without creating a replacement database, including when a mount-point directory still exists but its drive is absent. The flag is optional to allow normal first-run initialization. No external path is hardcoded, scanned or accessed automatically. No active database was moved to the pendrive. The settings row records the startup-configuration reference; environment selection is necessarily resolved before database settings can be read.

## Import, fingerprinting and replay

Game Library imports a selected file through the existing native zenity workflow or pasted text. File selection/read and parsing run off egui. Inputs must be regular UTF-8 files no larger than 16 MiB. The existing strict PGN/shakmaty parser validates each framed mainline; no illegal move is repaired. Framing recognizes termination tokens only outside tags, comments and variations, and retains trailing comments with the preceding game. Standard headers and custom FEN starts remain supported.

Partial-success policy: each valid game has its own transaction. It is inserted or skipped as a duplicate; parsing/storage errors are reported by game index and do not roll back previously successful games. Limits are 10,000 framed games, 200,000 cumulative validated positions and the existing 4,096 plies/game. The byte/game-count limits are checked before inserts; positions over the cumulative budget are reported and not inserted. Broken/unclosed delimiters cannot be safely resynchronized: the remaining fragment fails rather than guessing where another game begins. Counts/errors describe imported, skipped and failed games. Duplicate inserts never replace stored games.

Fingerprint = SHA-256 of normalized exported headers, initial state, canonical SAN and result. Formatting/comments do not affect it; materially different headers/results create a distinct record. Uniqueness is profile-scoped. Original sources preserve annotations where practical; canonical PGN and indexed moves are stored once per game, never full PGN per move. Standard optional metadata not promoted to query columns remains in canonical PGN headers. Clock fields are reserved but clock comments are not extracted yet.

Successful existing Game Review previews are also saved asynchronously in their originating profile. That accepted preview path retains strict all-document parsing/replacement behavior; Game Library is the partial-success importer. Explicit Save current game saves local games/variations as distinct content without replacing imported originals. Pending file operations retain their originating profile even if the selected profile changes.

Stored games load back through the same strict parser. Every persisted ply/SAN/UCI/FEN is compared with the reconstructed authoritative timeline before the record reaches Game Review. Corrupt/mismatched timelines return an error. Opening uses the existing board and indexed replay; local-work replacement still requires the existing confirmation. Persistent identity is attached only after the replacement is accepted.

## Library UI

Game Library displays players, date, result, time control, opening and whether any completed position analysis is cached. This status does not claim that the entire game was analyzed. It supports case-insensitive player/opening search, result and cached-analysis filters, newest/oldest date ordering, deterministic ID tie-breaking and pages of 50 rows. Queries/import/delete/maintenance are worker operations. Open loads the accepted existing Game Review implementation. Profile create/select/delete, PGN file/paste import and deletion confirmations use native egui controls. Other unrelated pages remain placeholders.

## Completed analysis cache

Cache identity includes:

- Stable stored game ID and ply.
- SHA-256 of the exact ordered complete FEN prefix, including clocks/rights/EP, with an explicit FEN-only history-isolation namespace.
- Verified live UCI engine identity including version.
- Executable setting, requested depth, MultiPV, Threads and Hash.
- Adapter version, fixed 30-second search budget, FEN-only protocol mode and optional advertised analyse-mode policy.

The current engine still receives **FEN only**, not preceding moves. Cached results describe that limited search, never a history-aware engine assessment. There is no global same-FEN reuse or sharing between games, repeated positions at different plies, profiles, engine versions, settings or budgets. Custom-FEN roots are non-reusable because pre-root history is unknown. A future history-aware UCI adapter must use a different protocol namespace/key; it must not inherit these entries as equivalent evaluations.

Only an enabled current-generation `bestmove` completion with a coherent snapshot **at exactly the requested depth** is offered for persistence. Stop, cancellation, errors, obsolete generations, partial/lower-depth groups and invalid PVs cannot become completed cache entries. MultiPV groups are atomic, correctly ranked, and reduced only when the root has fewer legal moves. Score types preserve cp versus mate winner/distance and exact/lower/upper bounds. Reads validate types, ranks, bounds and PV legality again. Malformed cache records return an error.

Automatic recording applies to an opened stored mainline; save/reopen a local game first. Read saved position analysis performs an explicit compatible lookup without starting Stockfish, with a selectable stored engine identity (default `Stockfish 17.1`). Saved scores and validated SAN PVs are labeled separately from live analysis. Start Analysis always performs a fresh live request; cached results do not impersonate live messages or automatically suppress an explicitly requested search. Asynchronous cache replies carry their original key and are displayed only while game/ply/settings/identity still match. Flipping affects neither identity nor scores.

## Persistent jobs

The tested API supports Pending → Running → Paused/Completed/Failed, Paused → Running, Failed → Running (increments retries), and explicit cancellation of unfinished jobs. Completed/Cancelled jobs do not restart. Completion requires all units finished. Progress must be monotonic, remain within total, and never rewind/clear an already recorded last position. Jobs cannot reference another profile's game.

Creating jobs and transitions/progress use transactions. A partial unique index rejects duplicate active work, including retries that would conflict with newer active work. Startup/normal worker shutdown convert leftover Running jobs to Paused with an interruption message, preserving progress and completed analysis. Resume is explicit through the API. There is intentionally **no scheduler or job-management UI and no automatic game/account Stockfish analysis**. Future scheduling should route the existing lifecycle operations through the storage worker; public lifecycle APIs are tested independently now.

## Settings and reliability

Validated Stockfish path/depth/MultiPV/Threads/Hash persist in a JSON preferences record; active profile persists through the unique active flag. Future sync interval (1–8760 hours) and startup-location reference also persist. The interval schedules no downloads. Invalid saved JSON/ranges fall back to safe defaults with a warning while retaining the original stored value. No credentials/secrets are stored. Engine settings are saved when valid edits change; failed writes show the database error and can be retried explicitly in library maintenance.

Local SSD policy: WAL, foreign keys ON, synchronous FULL, 750 ms busy timeout, checkpoint every 256 pages, journal size limit 4 MiB. The journal size limit is a retained-file target after checkpoints, not a strict maximum WAL size during a transaction. Lock/corruption errors remain recoverable and never trigger database deletion. Integrity maintenance runs both SQLite integrity and FK checks. Size inspection includes DB/WAL/SHM. Normal window close waits asynchronously for accepted work/settings and engine teardown, then signals storage shutdown/checkpoint/connection close. There is no UI thread join or database query. An unexpectedly disconnected worker is reported rather than leaving an endless busy state.

## Backup, restore and portability

Maintenance backup requires a new destination file; existing paths are refused rather than overwritten. Backups use SQLite's online backup API, not a live-file copy. Restore requires explicit confirmation, source integrity/current schema/application-ID checks, a distinct source, and a newly created safety backup of the current database. It then uses the backup API to replace the database and recovers running jobs. Copy steps have a 30-second deadline and return lock errors instead of retrying indefinitely. An incomplete destination backup is retained with an error, never advertised as complete. Close other Mfalmion instances before restore. The in-memory review is retained; reopen the intended restored library game afterward.

The [SQLite backup API](https://sqlite.org/c3ref/backup_finish.html) holds a destination write transaction; incomplete copying rolls back that transaction. [WAL documentation](https://sqlite.org/wal.html) describes its sidecars/shared-memory and local-host requirements. Do not copy only an active WAL-mode main file or remove its sidecars. Prefer the explicit single-file backup for transfer.

Portable preparation uses configurable/relative data paths and the optional require-existing-file guard. No portable launcher or exFAT deployment validation is implemented. **Do not move the active database to the pendrive yet.** SQLite safety depends on filesystem locking, durable writes and retained recovery files; surprise removal/power loss on removable storage can defeat those guarantees. This milestone's WAL policy is for the internal SSD. A future exFAT launcher needs exclusive application ownership, tested locking/sync behavior, a deliberate journal policy, backup discipline, proper close/checkpoint and safe unmount. exFAT presence alone does not establish those properties. See [SQLite corruption/locking guidance](https://sqlite.org/howtocorrupt.html). No filesystem, drive, mount or system settings were changed/tested here.

## Verification and storage impact

Required final checks:

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS, exit 0 |
| `cargo check --workspace` | PASS, exit 0 |
| `cargo test --workspace` | PASS, exit 0: **131 passed, 0 failed, 1 ignored** |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS, exit 0 |
| `git diff --check` | PASS, exit 0 |

42 additional tests cover initialization/migration/reopen/future-version rejection, profile selection/case matching/isolation/confirmed deletion, canonical duplicate imports, multi-game partial failure, comments/RAV/trailing annotations, invalid/oversized inputs, SQL-injection literal search, sort/filter/page boundaries, cascades, complete custom-FEN/castling reload/replay, malformed stored timelines/records, settings persistence/fallback, cache insert/read/type/bound/PV validation, depth/MultiPV/hash/version/budget/history separation, custom-FEN non-reuse, incomplete cancellation rejection, job progress/pause/resume/retry/crash recovery/duplicate isolation/rollback, integrity, backup/confirmed restore/corrupt restore, bounded lock/missing/corrupt database errors, unrelated/repository-local database rejection, background request/shutdown behavior and UI-independent identity/completion/profile regressions. All existing 89 tests still pass. The one ignored test is the existing real Stockfish smoke test; it was **not run** for this milestone. Existing CPU-only egui tests were included by cargo test; no native GUI automation was run.

Additional permitted smoke command:

```sh
cargo test --workspace storage::tests::sqlite_smoke_storage_impact -- --nocapture
```

Passed 1/1 using temporary files only. SQLite 3.54.1: empty database + sidecars **110,592 bytes**; one small game + completed analysis + job + settings **110,592 bytes**; standalone backup **77,824 bytes**. These are fixture measurements, not projections for thousands of full games/deep PVs. Test-owned temporary files are removed; no user's actual application DB was initialized or modified by tests.

Before compilation: approximately **7.6 GiB free**. After compilation/verification: approximately **7.4 GiB free** (7,964,626,944 bytes at the recorded check), target approximately **2.3 GiB**. Existing artifacts were reused, with no release build/cargo clean/cache cleanup. No model, SVG/binary asset or archive was added to SQLite. Cache groups upsert rather than retain every iterative search snapshot; game/profile deletion cascades cache/jobs. No automatic retention deletion/VACUUM is enabled. Future retention should offer explicit age/depth/configuration-aware cache cleanup and preview disk impact before reclaiming space. No full-account import/analysis or automatic downloading was performed.

## Files and manual acceptance

Created: `src/storage/{mod,schema,games,analysis,jobs,settings,worker,tests}.rs`, `src/ui/library.rs`, this report. Modified: Cargo.toml/Cargo.lock, `src/main.rs`, `src/app.rs`, `src/engine/mod.rs` (settings serialization only), `src/file_dialog.rs`, `src/ui/{mod,engine_panel,game_review}.rs`, README. Paths above are relative to `crates/mfalmion-app` except workspace lockfile, README and docs. Board/artwork/graphics/engine protocol/process/worker code is unchanged. Initial git status was clean; all milestone changes remain uncommitted/unpushed.

Manual X11 launch from a graphical terminal:

```sh
cd '/home/jairus/Antonius Jairus/Projects/Mfalmion'
env -u WAYLAND_DISPLAY MFALMION_WINDOW_SYSTEM=x11 VK_DRIVER_FILES=/usr/share/vulkan/icd.d/intel_icd.json WGPU_BACKEND=vulkan cargo run -p mfalmion-app --offline
```

- [ ] Check first-run/default profile and actual database location; import a small single/multi-game PGN in Game Library and confirm counts and duplicate skipping.
- [ ] Import a batch with valid/illegal/valid mainlines; confirm partial success and readable error. Invalid input must leave the current review intact.
- [ ] Search/filter/sort/page and reopen games; verify all replay keys/buttons, complete rights/EP/counters, board flipping, special moves, promotion, undo and original-mainline protection.
- [ ] Save a local variation; cancel opening a different library game, then confirm replacement. Verify persistent identity follows only accepted replacement.
- [ ] Create/select another profile; verify isolation. Cancel/confirm game and profile deletion and check only selected data disappears. Create another profile before deleting the final one.
- [ ] Change engine settings and future interval, close/reopen, verify persistence. A failed write must show its error; retry in maintenance.
- [ ] Open a standard-root stored game; complete a low-depth analysis, then read saved analysis with matching identity/settings. Navigate rapidly, change depth/MultiPV/identity and flip; stale/incompatible results must never be shown as current. Custom-FEN caches must not be reused.
- [ ] Run integrity/size; create a backup, refuse an existing backup name, cancel restore, then restore with a new safety path and reopen a game. Keep other application instances closed.
- [ ] Test an override with spaces and a missing parent. `MFALMION_REQUIRE_DATABASE=1` with an absent file must report an error and create no replacement database.
- [ ] Close during import/settings/cache operations and during engine analysis; reopen and check committed data/settings and no owned Stockfish leftover. Verify physical native rendering/layout, resize/focus, zenity cancellation and short-window scrolling.

Known limitations: native GUI acceptance is pending; job infrastructure has no scheduler/UI; custom-FEN-root cache reuse is deliberately disabled; cache reading is explicit and live searches remain fresh; cached/live engine searches retain Milestone 03's FEN-only repetition limitation; original annotations are retained but not edited and clocks are not extracted; local unsaved review state/cursor is not automatically restored across application restarts; no exFAT runtime guarantee or multi-instance job scheduler coordination is claimed. Database errors may require correcting the path/permissions and restarting; no automatic recovery deletes data.

Stop after Milestone 04. Wait for explicit acceptance before Milestone 05.
