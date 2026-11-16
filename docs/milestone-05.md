# Milestone 05 — Chess.com Published Data synchronization

Implemented 2026-11-16 after the user's acceptance of Milestone 04 native library/import/persistence testing. This milestone only: public username/profile resolution, archive discovery, incremental PGN ingestion, optional periodic updates and native sync controls. No full-account Stockfish analysis, move classifications, accuracy, Qwen, cloud services or Milestone 06. No GUI application, graphical automation, external chess archive access, system package installation, destructive cleanup, commit or push occurred.

## API and worker architecture

`sync/api.rs` handles usernames, expected endpoint/game URLs, UTC month selection and JSON/game validation. `client.rs` uses **ureq 3.5.2** with rustls certificate verification and gzip. `runner.rs` orchestrates discovery/import/checkpoints. `worker.rs` owns one serial sync operation, one reusable HTTP agent and one connection to the **same configured SQLite database**; it does not create a separate database. `ui/sync_panel.rs` provides native egui status/controls and scheduling. Existing library/storage worker, parser, indexed replay and UCI adapter are reused.

The separate sync worker lets the existing storage worker continue serving library queries/imports while network IO is pending. Both connections use WAL, foreign keys, bounded busy timeouts and short per-game transactions. No database connection/query/network call is created on each UI frame. Requests use a one-entry command channel and an atomic busy gate; progress uses one coalesced latest update under a short mutex, with a repaint callback. Only one sync runs per application instance. Automatic checks also use the busy gate. No webview, authentication flow, daemon or new engine process is involved.

Only HTTPS `api.chess.com/pub/player/{username}`, its archive index and exact monthly endpoints are requested. All redirects are refused, including unrelated hosts; no redirect is followed. User-Agent identifies Mfalmion/version and its project/contact URL. Requests are serial with at least one second between completed/next network calls. The global call deadline is 12 seconds, connection 5 seconds, per-read/write 2 seconds. Cancellation is checked before/after calls, between body chunks, archive months and games; a blocking network operation can take up to its deadline to finish cancelling.

Limits: 1 MiB decoded profile/index body, 32 MiB decoded monthly body, 1,200 index entries, 10,000 records per month, 200,000 validated snapshots per month, existing 16 MiB/4,096-ply PGN limits. One archive is processed/released before the next; no account-wide archive collection is retained. Unknown JSON fields are tolerated, required fields/types are not invented. Native offline tests use mocks and localhost HTTP fixtures; production URLs cannot use localhost/HTTP.

Protocol references: [official API](https://www.chess.com/news/view/published-data-api), [API usage guidance](https://support.chess.com/en/articles/9650547-what-is-the-pubapi-and-how-do-i-use-it), [ureq configuration](https://docs.rs/ureq/3.5.2/ureq/config/struct.ConfigBuilder.html). Requests respect Cache-Control (including no-cache/no-store), Age, Date, Expires, ETag/If-None-Match and Last-Modified/If-Modified-Since. Fresh cached responses avoid network calls; 304 reuses validated small profile/index JSON or a completed monthly checkpoint. Monthly JSON/PGN bodies are not stored in the HTTP cache. A 304 without usable prior data is an error.

HTTP 404 is reported as a missing profile/archive, 429 as throttling, 5xx as recoverable service failure; transport/timeout errors show Offline. Retry-After seconds and HTTP dates are respected, with a persisted database-wide cooldown that manual/full resync cannot bypass. No retry loop runs within an operation. A new operation can retry after the applicable cooldown/backoff. The official API can serve delayed/cached data; a 30-minute UI interval does not promise newly published data every 30 minutes.

## Usernames and identity

Matching is ASCII case-insensitive. Syntax requires at least three ASCII letters/digits/hyphens/underscores, alphanumeric first/last characters and at least one letter, following the [official username guidance](https://support.chess.com/en/articles/8609530-what-kind-of-username-can-i-choose). A local 100-byte safety cap matches profile storage; it is not presented as a claim about Chess.com's registration maximum. Invalid names produce an error without a request.

The public canonical username, optional player ID and display name are saved on the existing selected profile. Canonicalization can change casing only. A response resolving to a different account name or a changed known player ID is rejected instead of merging identities. Local profiles remain separate. No other known usernames are queried. The initial development profile remains `Antonius-Jairus-R`; selected-account sync requires no password, session cookie, API key or Chess.com authentication.

## SQLite schema v2

The additive v1 → v2 migration is transactional and ledgered; existing games/moves/analysis/jobs/settings are retained. New databases apply v1 then v2. Current v2 reopens do not recreate tables. Migration failure rolls back all v2 alterations. Version-1 backups remain restorable: validate source and create the required current safety backup, restore, then migrate the destination only. The backup source is unchanged.

| Addition | Stored data |
| --- | --- |
| profiles columns | Chess.com player ID, last sync error, next retry timestamp; existing canonical username/display name/last sync/status are reused. |
| remote_games | Profile-scoped unique external game URL, game FK, raw canonical PGN fingerprint, optional ratings, end timestamp, rated flag, raw player-result codes and API ECO/opening metadata. Composite FK prevents cross-profile links. |
| archive_sync | Profile/month checkpoint, pending/complete/complete_with_errors, per-month imported/skipped/failed counts, checked timestamp. |
| sync_http | Profile/endpoint validators, freshness deadline and optional small profile/index JSON. No monthly PGN payload copies. |
| sync_runs | Latest run progress/status/counts/error/timestamps per profile. No unbounded run-log accumulation. |
| sync_failures | At most 100 bounded (2,000-character) item errors per profile/month; all failed items still contribute to counts. Details are replaced when that month's body is retried. |
| settings rows | Validated enabled/interval preferences and shared HTTP cooldown. Legacy engine preferences are preserved. |

Game metadata is taken from the validated PGN and available JSON. Stored fields include external URL, case-preserved players, ratings, PGN result, time class/control, end timestamp, rated/casual flag, ECO/opening. API player result codes are retained independently; they do not override authoritative PGN results. ECO URLs are retained as URLs rather than invented ECO codes. Missing values stay missing. Existing nonempty metadata is retained; missing query fields can be enriched without replacing canonical/original PGN.

SQL values are parameterized; multi-record inserts/checkpoints use transactions and FKs/cascades. `games.rs` exposes an internal validated transaction helper so game, move timeline, source-document and external metadata insert together. The manual importer still uses the same helper and retains all previous behavior. No engine cache/job records are reset by sync.

## Deduplication and local work

Normalized Chess.com live/daily game URLs are the primary external identifiers, unique within a profile. Existing manual Link URLs and exact canonical fingerprints are matched where safe. The first stored original/annotated PGN is never silently replaced. Existing evaluations, complete analysis jobs and game IDs remain intact.

The canonical SHA-256 fingerprint remains available for content matching. Distinct known external IDs must not collapse merely because their PGNs are identical: such records use an external-identity-qualified storage fingerprint while retaining the raw canonical hash in remote metadata. Tests exercise repeated syncs of both IDs. Manual imports lacking a stable URL and differing metadata are not aggressively matched using just moves/players/date; ambiguous matches are left separate rather than merging unrelated games. Sync does not rewrite local variations or modify a currently open review.

## Initial/incremental sync and partial success

Sync Now validates/resolves the selected public account, retrieves and validates the archive index, deduplicates/sorts actual months chronologically, then processes them serially. Unexpected hosts, account paths, query strings, fragments, ports or endpoint shapes are rejected. Missing months are not invented. Empty accounts/months finish normally.

Each valid standard-chess game is independently validated by the existing PGN/shakmaty infrastructure, then inserted transactionally or skipped as a duplicate. Invalid/missing PGNs, metadata mismatches and unsupported variants are rejected with counts and bounded stored errors; no illegal move is repaired. The UI includes a representative error and failure count. A month becomes complete/complete_with_errors only after every returned record has been attempted. This means a partial-success sync can finish with rejected games, clearly labeled, and still record the successful check timestamp. Rejected PGNs are not duplicated in the database; retry through Full Resync when the response becomes eligible again. Detailed per-item errors currently reside in `sync_failures`; the panel shows a summary/example.

Network/JSON/database/cancellation failures abort unfinished work rather than treating it as a completed month. Successful game commits remain. Beginning a replacement monthly body marks the checkpoint pending and invalidates its previous HTTP validators; cancellation during Full Resync therefore cannot leave an old completion flag hiding unfinished new records. A later sync retrieves/revalidates that month and skips already inserted games safely. A database lock never becomes a permanent rejected-game completion.

After initial sync, completed historical months are skipped. Newly discovered or pending months are fetched. Current and previous **UTC** months are rechecked when present in the actual index, respecting freshness/conditional responses. Full Resync revisits all discovered months, without bypassing freshness/cooldowns or overwriting local PGNs. An unchanged/fresh month preserves its existing failed-item counts/status rather than erasing partial-success warnings. Progress counts skipped retained games as skipped; rejected retained records remain visible in failed totals.

## Automatic checks, cancellation and lifecycle

Auto Sync is **enabled by default every 30 minutes**. It is configurable from 5 to 10,080 minutes (7 days), validated and persisted separately from engine settings. The old reserved hours preference is retained for compatibility but is not used by the scheduler. The first automatic check waits one configured interval after launch/profile selection; opening Mfalmion does not require immediate internet access. Auto off means manual-only sync; there is no closed-app daemon.

The scheduler arms a future egui wake rather than continuously polling. Transient/Offline failures defer further work using bounded exponential backoff (2, 4, 8, 16, then 30 minutes), extended by server cooldowns. These scheduled probes permit recovery after connectivity returns. Permanent 4xx errors pause automatic work for that active schedule until an explicit retry/profile change. Cancel preserves work and does not disable Auto Sync; the next automatic check is scheduled at the normal configured interval. Disable Auto Sync as well to stop future checks.

Cancel/shutdown use atomic signals and never wait for network on egui. Normal close waits asynchronously for the sync worker/HTTP deadline and its connection teardown alongside existing engine/storage shutdown. Startup converts persisted Running/Syncing state to interrupted status with checkpoints retained; no automatic account-wide Stockfish job is launched. Resume reuses the actual discovered months/checkpoints instead of trusting a lost in-memory cursor. Profile switching cancels prior work, whose immutable original profile ID prevents misassociation. Restore/profile deletion require cancelling/waiting for active sync first. Existing manual import, search/filter/sort/page/open/export and engine analysis continue through their existing components during network work.

The SQLite busy timeout remains 750 ms. Two same-database worker connections permit concurrent reads; lock/write errors abort safely and can be retried. This milestone guarantees one sync per application instance, not a distributed/multi-process sync lease; run one Mfalmion instance against a given database. Backups remain safe via the existing SQLite online backup API. Extreme/corrupt retry timestamps are bounded in UI formatting/wake calculations rather than causing a panic.

## Verification

Final required checks:

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS, exit 0 |
| `cargo check --workspace` | PASS, exit 0 |
| `cargo test --workspace` | PASS, exit 0 — **170 passed, 0 failed, 2 ignored** |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS, exit 0 |
| `git diff --check` | PASS, exit 0 |

39 new passing tests cover username/profile identity, strict archive URLs, sorted/deduplicated discovery, empty accounts/months, PGN/metadata/variant checks, primary/fingerprint/distinct-ID deduplication, initial/incremental/new-month/current/previous refresh, full resync, conditional 304/freshness, partial success/error retention, cancellation/restart/checkpoints, 404/429/5xx/Retry-After, Offline/timeout behavior, malformed responses, scheduling/disable/backoff/no overlap, settings/profile isolation/persistence, cache/job preservation, bounded errors, additive migration/rollback/old-backup restore, real localhost headers/redirect refusal/body limit/timeout, worker shutdown, transient DB locks and extreme timestamps. All previous 131 tests remain passing (migration assertions now expect v2). The two ignored tests are opt-in live Chess.com and existing real Stockfish smoke tests. No native GUI test/automation was run; existing headless CPU egui tests remain part of Cargo's suite.

After offline checks passed, the permitted smoke test ran:

```sh
cargo test --workspace sync::tests::live_chesscom_smoke -- --ignored --nocapture
```

**LIVE SMOKE PASSED**, test exit 0, 1 passed, 0 failed. Exactly three serial HTTPS requests: public profile, archive index, latest monthly archive. Canonical account `antonius-jairus-r`, player ID `619352270`; one discovered month, **2026/11**, containing **149 games**. One supported PGN was validated, **115 plies**, URL `https://www.chess.com/game/live/186794618804`. This validates one PGN, not all 149 records or full-account import. No database was opened/written by this smoke test; the real application database and external 729-game archive were untouched. No Stockfish/Qwen process was started by milestone validation (existing mock UCI tests run only their owned fixtures).

## Files, SSD and limitations

Created: `src/sync/{mod,api,client,runner,worker,tests}.rs`, `src/storage/sync.rs`, `src/ui/sync_panel.rs`, this report. Modified: Cargo.toml/Cargo.lock, README, `src/{main,app}.rs`, `src/storage/{mod,schema,games,settings,tests,worker}.rs`, `src/ui/{mod,library}.rs`. Source paths are relative to `crates/mfalmion-app`; Cargo.lock/README/docs are workspace-level. Board/artwork/animation/rules/replay/engine implementation files are unchanged.

Before compilation: **7,891,804,160 bytes** free, approximately **7.4 GiB**. After verification: approximately **7.2 GiB** free; target approximately **2.5 GiB**. New locked dependencies are ureq/rustls transport and httpdate, reusing existing compression/serde/SQLite. No system package, model file, engine binary, release build, cargo clean, unrelated filesystem write or external-drive operation. Tests use owned temporary SQLite databases only; the actual application DB has not been migrated by these tests. Runtime migration happens when the user next launches the app.

Initial git status was clean. All Milestone 05 source/docs changes remain local, uncommitted and unpushed. No Milestone 06 work started.

Known limitations: native GUI layout, real cancel/close interaction and production migration acceptance remain manual; standard-chess PGNs only; service cache delays can exceed the configured interval; no account rename/identity merge workflow; manual imports with ambiguous metadata are not heuristically merged; rejected-item details have no dedicated browser yet; HTTP cancellation is bounded by an in-flight timeout; only one application instance per DB is supported for sync coordination; auto-sync backoff is in-memory while server cooldowns, progress and archive checkpoints persist. Closing/reopening restarts scheduling from the configured interval and persisted cooldowns. No full-account live import was performed during development.

## Manual acceptance

```sh
cd '/home/jairus/Antonius Jairus/Projects/Mfalmion'
env -u WAYLAND_DISPLAY MFALMION_WINDOW_SYSTEM=x11 VK_DRIVER_FILES=/usr/share/vulkan/icd.d/intel_icd.json WGPU_BACKEND=vulkan cargo run -p mfalmion-app --offline
```

- [ ] Back up the existing database through maintenance; confirm v1 data/games/cache/jobs survive v2 startup and restart.
- [ ] In Game Library check selected account, canonical name/status, progress/counts and readable errors. Sync Now for the selected account only; repeat and verify no duplicates or PGN/evaluation/job loss.
- [ ] Verify existing manual PGNs with matching Link/fingerprint are reused. Open synced games in the existing review; test replay, legal special moves, promotion, animation, flipping/resizing and unchanged engine analysis.
- [ ] Check search/filter/sort/page/manual PGN import/export during downloads; the window and library must remain responsive.
- [ ] Cancel during download and record processing, then resume/restart. Completed games/months must persist and unfinished months must retry safely.
- [ ] Test a nonexistent/invalid username and Offline failure; existing games/review/Stockfish must remain usable. Switch profiles, confirm isolation, and wait for cancellation before destructive profile/restore operations.
- [ ] Disable Auto Sync and restart; verify preference persistence. Enable a short test interval and confirm only one operation starts, errors back off and server cache/Retry-After is respected. Restore 30 minutes afterward if desired.
- [ ] Full Resync must revisit eligible historical archives without overwriting local work, bypassing cache freshness or launching Stockfish.
- [ ] Close while syncing/reading and while analyzing; allow bounded teardown, reopen and verify persisted status/checkpoints/data. No sync daemon should remain after exit.

Stop after Milestone 05. Wait for explicit user acceptance before Milestone 06.
