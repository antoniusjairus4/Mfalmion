use super::*;
use crate::{
    engine::{
        Settings,
        protocol::{Bound, Info, Score, Snapshot},
    },
    notation,
};
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp {
    root: PathBuf,
}
impl Temp {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "mfalmion-storage-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        Self { root }
    }
    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
    fn store(&self) -> Store {
        Store::open(&self.path("test.sqlite3")).unwrap()
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
const PGN: &str = "[White \"Alice\"]\n[Black \"Bob\"]\n[Date \"2026.11.16\"]\n[Opening \"Italian\"]\n\n1.e4 e5 2.Nf3 Nc6 *";
fn seed(s: &mut Store) -> (i64, i64) {
    let p = s.active_profile().unwrap();
    assert!(
        s.insert_game(p, &notation::parse_pgn(PGN).unwrap()[0], Some(PGN))
            .unwrap()
    );
    let id = s.list_games(p, &Default::default()).unwrap()[0].id;
    (p, id)
}
fn fixture(s: &mut Store) -> (analysis::Key, Snapshot) {
    let (p, id) = seed(s);
    let r = s.load_game(p, id).unwrap();
    let settings = Settings {
        depth: 6,
        multipv: 1,
        ..Default::default()
    };
    let key = analysis::Key::new(id, &r, 0, "Stockfish 17.1", &settings).unwrap();
    let snapshot = Snapshot {
        depth: 6,
        lines: vec![Info {
            depth: Some(6),
            seldepth: Some(8),
            multipv: 1,
            nodes: Some(42),
            nps: None,
            time_ms: Some(1),
            score: Some(Score::Cp(25)),
            bound: Bound::Lower,
            pv: vec!["e2e4".into(), "e7e5".into()],
        }],
    };
    (key, snapshot)
}
#[test]
fn initialization_and_migration() {
    let t = Temp::new();
    let s = t.store();
    assert_eq!(s.integrity().unwrap(), "ok");
    assert_eq!(
        s.db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        s.db.query_row("SELECT count(*) FROM schema_migrations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(s.size().unwrap() > 0);
}
#[test]
fn reopening_preserves_state() {
    let t = Temp::new();
    {
        let mut s = t.store();
        seed(&mut s);
    }
    let s = t.store();
    assert_eq!(
        s.list_games(s.active_profile().unwrap(), &Default::default())
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        s.db.query_row("SELECT count(*) FROM schema_migrations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
#[test]
fn future_schema_rejected() {
    let t = Temp::new();
    let s = t.store();
    s.db.pragma_update(None, "user_version", 99).unwrap();
    drop(s);
    assert!(Store::open(&t.path("test.sqlite3")).is_err());
}
#[test]
fn profiles_case_insensitive_and_selection_persists() {
    let t = Temp::new();
    let mut s = t.store();
    let a = s.select_profile("Some-Player").unwrap();
    assert_eq!(a, s.select_profile("some-player").unwrap());
    assert_eq!(s.profiles().unwrap().len(), 2);
    drop(s);
    assert_eq!(t.store().active_profile().unwrap(), a);
}
#[test]
fn profile_isolation_and_confirmed_delete() {
    let t = Temp::new();
    let mut s = t.store();
    let (p, id) = seed(&mut s);
    let other = s.select_profile("Other").unwrap();
    assert!(s.list_games(other, &Default::default()).unwrap().is_empty());
    assert!(s.load_game(other, id).is_err());
    assert!(s.delete_profile(p, false).is_err());
    s.delete_profile(p, true).unwrap();
    assert_eq!(s.active_profile().unwrap(), other);
    assert!(s.load_game(p, id).is_err());
    assert!(s.delete_profile(other, true).is_err());
    assert_eq!(s.integrity().unwrap(), "ok");
}
#[test]
fn duplicate_game_never_replaces() {
    let t = Temp::new();
    let mut s = t.store();
    let (p, id) = seed(&mut s);
    assert!(
        !s.insert_game(
            p,
            &notation::parse_pgn(PGN).unwrap()[0],
            Some("changed source")
        )
        .unwrap()
    );
    assert_eq!(
        s.load_game(p, id).unwrap().original_pgn.as_deref(),
        Some(PGN)
    );
    let other = s.select_profile("Other").unwrap();
    assert!(
        s.insert_game(other, &notation::parse_pgn(PGN).unwrap()[0], None)
            .unwrap()
    );
}
#[test]
fn multigame_partial_import() {
    let t = Temp::new();
    let mut s = t.store();
    let p = s.active_profile().unwrap();
    let report = s
        .import_pgn(p, &format!("{PGN}\n\n1.e5 *\n\n1.d4 d5 *"))
        .unwrap();
    assert_eq!(
        (report.imported, report.skipped, report.errors.len()),
        (2, 0, 1)
    );
    let again = s.import_pgn(p, PGN).unwrap();
    assert_eq!(again.skipped, 1);
    assert_eq!(s.list_games(p, &Default::default()).unwrap().len(), 2);
}
#[test]
fn fragments_respect_comments_tags_variations() {
    let text = "[Event \"1-0\"]\n1.e4 {0-1} (1.d4 1-0) e5 *\n1.d4 ; 0-1\nd5 *";
    assert_eq!(games::fragments(text).len(), 2);
    let t = Temp::new();
    let mut s = t.store();
    let p = s.active_profile().unwrap();
    assert_eq!(s.import_pgn(p, text).unwrap().imported, 2);
}
#[test]
fn invalid_import_keeps_existing() {
    let t = Temp::new();
    let mut s = t.store();
    let (p, id) = seed(&mut s);
    assert_eq!(s.import_pgn(p, "1.e5 *").unwrap().errors.len(), 1);
    assert!(
        s.import_pgn(p, &" ".repeat(notation::MAX_PGN_BYTES + 1))
            .is_err()
    );
    assert_eq!(s.load_game(p, id).unwrap().moves.len(), 4);
}
#[test]
fn search_filter_sort_and_pagination() {
    let t = Temp::new();
    let mut s = t.store();
    let (p, _) = seed(&mut s);
    s.import_pgn(p, "[Date \"2000.01.01\"]\n1.d4 1-0").unwrap();
    let mut q = games::Query {
        search: "alice".into(),
        ..Default::default()
    };
    assert_eq!(s.list_games(p, &q).unwrap().len(), 1);
    q.search = "' OR 1=1 --".into();
    assert!(s.list_games(p, &q).unwrap().is_empty());
    q.search.clear();
    q.result = "1-0".into();
    assert_eq!(s.list_games(p, &q).unwrap().len(), 1);
    q.result.clear();
    q.oldest = true;
    assert_eq!(s.list_games(p, &q).unwrap()[0].date, "2000.01.01");
    q.offset = 50;
    assert!(s.list_games(p, &q).unwrap().is_empty());
}
#[test]
fn deletion_cascades_moves_cache_jobs() {
    let t = Temp::new();
    let mut s = t.store();
    let (key, snapshot) = fixture(&mut s);
    let p = s.active_profile().unwrap();
    s.cache_analysis(&key, &snapshot, true).unwrap();
    s.create_job(p, Some(key.game), "test", "key", 4).unwrap();
    assert!(s.delete_game(p, key.game, false).is_err());
    s.delete_game(p, key.game, true).unwrap();
    for table in ["game_moves", "analysis", "jobs", "documents"] {
        assert_eq!(
            s.db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    assert_eq!(s.integrity().unwrap(), "ok");
}
#[test]
fn complete_reload_preserves_timeline_and_fen() {
    let t = Temp::new();
    let mut s = t.store();
    let p = s.active_profile().unwrap();
    let source = "[SetUp \"1\"]\n[FEN \"r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 7 40\"]\n40.O-O O-O-O *";
    s.import_pgn(p, source).unwrap();
    let id = s.list_games(p, &Default::default()).unwrap()[0].id;
    let r = s.load_game(p, id).unwrap();
    let original = notation::parse_pgn(source).unwrap().remove(0);
    for i in [0, 1, 2, 1, 0, 2] {
        assert_eq!(
            notation::fen(&r.positions[i]),
            notation::fen(&original.positions[i])
        );
    }
    assert_eq!(r.played_moves, original.played_moves);
    assert_eq!(r.result, "*");
}
#[test]
fn corrupt_timeline_returns_error() {
    let t = Temp::new();
    let mut s = t.store();
    let (p, id) = seed(&mut s);
    s.db.execute(
        "UPDATE game_moves SET fen='invalid' WHERE game_id=?1 AND ply=1",
        [id],
    )
    .unwrap();
    assert!(s.load_game(p, id).is_err());
}
#[test]
fn settings_survive_reopening() {
    let t = Temp::new();
    let s = t.store();
    let mut p = settings::Preferences::default();
    p.engine.depth = 12;
    p.engine.path = "/tmp/engine with spaces".into();
    p.sync_interval_hours = 48;
    s.save_preferences(&p).unwrap();
    drop(s);
    assert_eq!(t.store().preferences().unwrap().0, p);
}
#[test]
fn invalid_settings_fallback_retains_content() {
    let t = Temp::new();
    let s = t.store();
    s.db.execute(
        "INSERT INTO settings(key,value) VALUES('preferences','broken JSON')",
        [],
    )
    .unwrap();
    let (p, warning) = s.preferences().unwrap();
    assert_eq!(p, settings::Preferences::default());
    assert!(warning.is_some());
    let mut p = p;
    p.engine.depth = 100;
    assert!(s.save_preferences(&p).is_err());
    assert_eq!(
        s.db.query_row(
            "SELECT value FROM settings WHERE key='preferences'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "broken JSON"
    );
}
#[test]
fn cache_roundtrip_preserves_score_bound_pv() {
    let t = Temp::new();
    let mut s = t.store();
    let (key, snapshot) = fixture(&mut s);
    s.cache_analysis(&key, &snapshot, true).unwrap();
    let lines = s.cached_analysis(&key).unwrap().unwrap();
    assert_eq!(lines[0].score, analysis::CachedScore::Cp(25));
    assert_eq!(lines[0].bound, "lower");
    assert_eq!(lines[0].pv, snapshot.lines[0].pv);
    drop(s);
    assert!(t.store().cached_analysis(&key).unwrap().is_some());
}
#[test]
fn cache_depth_multipv_engine_separation() {
    let t = Temp::new();
    let mut s = t.store();
    let (key, snapshot) = fixture(&mut s);
    s.cache_analysis(&key, &snapshot, true).unwrap();
    let p = s.active_profile().unwrap();
    let r = s.load_game(p, key.game).unwrap();
    for settings in [
        Settings {
            depth: 7,
            multipv: 1,
            ..Default::default()
        },
        Settings {
            depth: 6,
            multipv: 2,
            ..Default::default()
        },
        Settings {
            depth: 6,
            multipv: 1,
            hash_mib: 32,
            ..Default::default()
        },
    ] {
        let other = analysis::Key::new(key.game, &r, 0, &key.engine, &settings).unwrap();
        assert!(s.cached_analysis(&other).unwrap().is_none());
    }
    let mut other = key.clone();
    other.engine = "Stockfish 18".into();
    assert!(s.cached_analysis(&other).unwrap().is_none());
}
#[test]
fn cache_rejects_incomplete_and_invalid_groups() {
    let t = Temp::new();
    let mut s = t.store();
    let (key, mut snapshot) = fixture(&mut s);
    assert!(s.cache_analysis(&key, &snapshot, false).is_err());
    snapshot.depth = 5;
    assert!(s.cache_analysis(&key, &snapshot, true).is_err());
    snapshot.depth = 6;
    snapshot.lines[0].multipv = 2;
    assert!(s.cache_analysis(&key, &snapshot, true).is_err());
    snapshot.lines[0].multipv = 1;
    snapshot.lines[0].pv = vec!["e2e5".into()];
    assert!(s.cache_analysis(&key, &snapshot, true).is_err());
    assert!(s.cached_analysis(&key).unwrap().is_none());
}
#[test]
fn cache_context_history_isolation() {
    let t = Temp::new();
    let mut s = t.store();
    let (key, snapshot) = fixture(&mut s);
    s.cache_analysis(&key, &snapshot, true).unwrap();
    let p = s.active_profile().unwrap();
    let original = s.load_game(p, key.game).unwrap();
    let mut repeated = notation::parse_pgn("1.Nf3 Nf6 2.Ng1 Ng8 *")
        .unwrap()
        .remove(0);
    s.insert_game(p, &repeated, None).unwrap();
    let id = s
        .list_games(p, &Default::default())
        .unwrap()
        .iter()
        .find(|r| r.id != key.game)
        .unwrap()
        .id;
    let settings = Settings {
        depth: 6,
        multipv: 1,
        ..Default::default()
    };
    let other = analysis::Key::new(id, &repeated, 4, &key.engine, &settings).unwrap();
    assert_ne!(key.context, other.context);
    assert!(s.cached_analysis(&other).unwrap().is_none());
    repeated.positions[4] = original.positions[0].clone();
    let forged = analysis::Key::new(id, &repeated, 4, &key.engine, &settings).unwrap();
    assert!(s.cached_analysis(&forged).is_err());
}
#[test]
fn mate_cache_remains_distinct() {
    let t = Temp::new();
    let mut s = t.store();
    let (key, mut snapshot) = fixture(&mut s);
    snapshot.lines[0].score = Some(Score::Mate {
        winner: shakmaty::Color::Black,
        moves: 3,
    });
    snapshot.lines[0].bound = Bound::Upper;
    s.cache_analysis(&key, &snapshot, true).unwrap();
    assert_eq!(
        s.cached_analysis(&key).unwrap().unwrap()[0].score,
        analysis::CachedScore::Mate {
            white: false,
            moves: 3
        }
    );
}
#[test]
fn job_lifecycle_progress_and_retry() {
    let t = Temp::new();
    let mut s = t.store();
    let (p, id) = seed(&mut s);
    let j = s.create_job(p, Some(id), "positions", "game:1", 4).unwrap();
    assert_eq!(s.job(j).unwrap().status, jobs::Status::Pending);
    s.transition_job(j, jobs::Status::Running, None).unwrap();
    s.progress_job(j, 2, Some(1)).unwrap();
    s.transition_job(j, jobs::Status::Paused, None).unwrap();
    assert!(s.progress_job(j, 3, Some(2)).is_err());
    s.transition_job(j, jobs::Status::Running, None).unwrap();
    s.transition_job(j, jobs::Status::Failed, Some("test failure"))
        .unwrap();
    s.transition_job(j, jobs::Status::Running, None).unwrap();
    assert_eq!(s.job(j).unwrap().retries, 1);
    assert!(s.transition_job(j, jobs::Status::Completed, None).is_err());
    s.progress_job(j, 4, Some(3)).unwrap();
    s.transition_job(j, jobs::Status::Completed, None).unwrap();
    let job = s.job(j).unwrap();
    assert_eq!(
        (job.id, job.total, job.completed, job.last_position),
        (j, 4, 4, Some(3))
    );
    assert!(job.last_error.is_none());
    assert!(s.transition_job(j, jobs::Status::Running, None).is_err());
}
#[test]
fn crash_recovery_preserves_progress() {
    let t = Temp::new();
    let j;
    {
        let mut s = t.store();
        let p = s.active_profile().unwrap();
        j = s.create_job(p, None, "test", "crash", 10).unwrap();
        s.transition_job(j, jobs::Status::Running, None).unwrap();
        s.progress_job(j, 3, Some(2)).unwrap();
    }
    let s = t.store();
    assert_eq!(s.recover_jobs().unwrap(), 1);
    let job = s.job(j).unwrap();
    assert_eq!(job.status, jobs::Status::Paused);
    assert_eq!(job.completed, 3);
    assert!(job.last_error.is_some());
    assert_eq!(s.recover_jobs().unwrap(), 0);
}
#[test]
fn duplicate_jobs_prevented_and_crossprofile_rejected() {
    let t = Temp::new();
    let mut s = t.store();
    let (p, id) = seed(&mut s);
    let j = s.create_job(p, Some(id), "test", "same", 1).unwrap();
    assert!(s.create_job(p, Some(id), "test", "same", 1).is_err());
    s.transition_job(j, jobs::Status::Cancelled, None).unwrap();
    assert!(s.create_job(p, Some(id), "test", "same", 1).is_ok());
    let other = s.select_profile("Other").unwrap();
    assert!(s.create_job(other, Some(id), "test", "same", 1).is_err());
}
#[test]
fn transaction_rollback_leaves_no_orphan_document() {
    let t = Temp::new();
    let mut s = t.store();
    assert!(
        s.insert_game(999, &notation::parse_pgn(PGN).unwrap()[0], Some(PGN))
            .is_err()
    );
    assert_eq!(
        s.db.query_row("SELECT count(*) FROM documents", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(s.integrity().unwrap(), "ok");
}
#[test]
fn backup_restore_requires_confirmation_and_new_safety_file() {
    let t = Temp::new();
    let mut s = t.store();
    let (p, id) = seed(&mut s);
    let backup = t.path("backup.sqlite3");
    s.backup(&backup).unwrap();
    assert!(s.backup(&backup).is_err());
    s.delete_game(p, id, true).unwrap();
    let safety = t.path("safety.sqlite3");
    assert!(s.restore(&backup, &safety, false).is_err());
    s.restore(&backup, &safety, true).unwrap();
    assert!(s.load_game(p, id).is_ok());
    assert!(safety.exists());
    assert!(s.restore(&backup, &safety, true).is_err());
    assert!(
        s.restore(&t.path("test.sqlite3"), &t.path("another"), true)
            .is_err()
    );
}
#[test]
fn corrupt_restore_cannot_replace_live_database() {
    let t = Temp::new();
    let mut s = t.store();
    let (p, id) = seed(&mut s);
    let bad = t.path("bad.sqlite3");
    std::fs::write(&bad, b"not sqlite").unwrap();
    assert!(s.restore(&bad, &t.path("safety"), true).is_err());
    assert!(s.load_game(p, id).is_ok());
    assert!(!t.path("safety").exists());
}
#[test]
fn missing_locked_corrupt_database_errors_are_bounded() {
    let t = Temp::new();
    assert!(Store::open(&t.path("missing/test.sqlite3")).is_err());
    let mut s = t.store();
    let (p, _) = seed(&mut s);
    let other = Connection::open(&s.path).unwrap();
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    let started = std::time::Instant::now();
    assert!(s.import_pgn(p, "1.d4 *").unwrap().errors.len() == 1);
    assert!(started.elapsed() < Duration::from_secs(3));
    other.execute_batch("ROLLBACK").unwrap();
    let bad = t.path("bad.sqlite3");
    std::fs::write(&bad, b"corrupt").unwrap();
    assert!(Store::open(&bad).is_err());
    assert_eq!(std::fs::read(&bad).unwrap(), b"corrupt");
}
#[test]
fn cache_malformed_json_returns_error() {
    let t = Temp::new();
    let mut s = t.store();
    let (key, snapshot) = fixture(&mut s);
    s.cache_analysis(&key, &snapshot, true).unwrap();
    s.db.execute("UPDATE analysis SET lines='not json'", [])
        .unwrap();
    assert!(s.cached_analysis(&key).is_err());
}

#[test]
fn background_worker_roundtrip_and_shutdown() {
    use worker::{Client, Command, Reply};
    let t = Temp::new();
    let client = Client::at(t.path("worker.sqlite3")).unwrap();
    let profile = match client
        .receiver
        .recv_timeout(Duration::from_secs(3))
        .unwrap()
    {
        Reply::State { active, .. } => active,
        _ => panic!("Expected initialized state"),
    };
    client.send(Command::Import(profile, PGN.into())).unwrap();
    assert!(matches!(
        client
            .receiver
            .recv_timeout(Duration::from_secs(3))
            .unwrap(),
        Reply::Imported(games::ImportReport { imported: 1, .. })
    ));
    client
        .send(Command::Refresh(profile, Default::default()))
        .unwrap();
    let id = match client
        .receiver
        .recv_timeout(Duration::from_secs(3))
        .unwrap()
    {
        Reply::Games(rows) => rows[0].id,
        _ => panic!("Expected library rows"),
    };
    client.send(Command::Open(profile, id)).unwrap();
    assert!(
        matches!(client.receiver.recv_timeout(Duration::from_secs(3)).unwrap(),Reply::Opened(p,g,_) if p==profile&&g==id)
    );
    client.shutdown();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while !client.done.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(client.done.load(Ordering::Acquire));
    let s = Store::open(&t.path("worker.sqlite3")).unwrap();
    assert_eq!(s.load_game(profile, id).unwrap().moves.len(), 4);
}
#[test]
fn background_worker_missing_directory_reports_error() {
    let t = Temp::new();
    let client = worker::Client::at(t.path("absent/db.sqlite3")).unwrap();
    assert!(matches!(
        client
            .receiver
            .recv_timeout(Duration::from_secs(3))
            .unwrap(),
        worker::Reply::Error(_)
    ));
}
#[test]
fn inconsistent_record_returns_error_without_panic() {
    let t = Temp::new();
    let mut s = t.store();
    let mut record = notation::GameRecord::default();
    record.positions.clear();
    assert!(
        s.insert_game(s.active_profile().unwrap(), &record, None)
            .is_err()
    );
}
#[test]
fn unrelated_database_is_not_migrated() {
    let t = Temp::new();
    let path = t.path("unrelated.sqlite3");
    let db = Connection::open(&path).unwrap();
    db.execute("CREATE TABLE personal_data(value TEXT)", [])
        .unwrap();
    drop(db);
    assert!(Store::open(&path).is_err());
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "delete"
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name='profiles'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}
#[test]
fn pagination_fifty_rows_and_date_sort_are_stable() {
    let t = Temp::new();
    let mut s = t.store();
    let p = s.active_profile().unwrap();
    let text = (0..61)
        .map(|i| format!("[Event \"Game {i}\"]\n1.e4 *\n\n"))
        .collect::<String>();
    assert_eq!(s.import_pgn(p, &text).unwrap().imported, 61);
    let q = games::Query::default();
    let first = s.list_games(p, &q).unwrap();
    let second = s.list_games(p, &games::Query { offset: 50, ..q }).unwrap();
    assert_eq!((first.len(), second.len()), (50, 11));
    assert!(first.iter().all(|a| second.iter().all(|b| a.id != b.id)));
}
#[test]
fn job_progress_never_rewinds_last_position() {
    let t = Temp::new();
    let mut s = t.store();
    let p = s.active_profile().unwrap();
    let id = s.create_job(p, None, "test", "progress", 10).unwrap();
    s.transition_job(id, jobs::Status::Running, None).unwrap();
    s.progress_job(id, 5, Some(4)).unwrap();
    assert!(s.progress_job(id, 6, Some(3)).is_err());
    assert!(s.progress_job(id, 4, Some(4)).is_err());
    assert_eq!(s.job(id).unwrap().completed, 5);
}
#[test]
fn cache_rejects_unknown_bound_and_changed_adapter_budget() {
    let t = Temp::new();
    let mut s = t.store();
    let (key, snapshot) = fixture(&mut s);
    s.cache_analysis(&key, &snapshot, true).unwrap();
    let mut changed = key.clone();
    changed.configuration = changed.configuration.replace("30000", "1000");
    assert!(s.cached_analysis(&changed).is_err());
    let mut lines = s.cached_analysis(&key).unwrap().unwrap();
    lines[0].bound = "arbitrary".into();
    s.db.execute(
        "UPDATE analysis SET lines=?1",
        [serde_json::to_string(&lines).unwrap()],
    )
    .unwrap();
    assert!(s.cached_analysis(&key).is_err());
}
#[test]
fn final_comments_do_not_create_phantom_game() {
    let t = Temp::new();
    let mut s = t.store();
    let p = s.active_profile().unwrap();
    let text = "1.e4 {during} e5 * {after result} ; final note\n";
    let report = s.import_pgn(p, text).unwrap();
    assert_eq!((report.imported, report.errors.len()), (1, 0));
    let id = s.list_games(p, &Default::default()).unwrap()[0].id;
    assert_eq!(
        s.load_game(p, id).unwrap().original_pgn.as_deref(),
        Some(text)
    );
}
#[test]
fn custom_fen_history_is_not_reusable() {
    let r = notation::GameRecord::new(notation::parse_fen("7k/8/8/8/8/8/8/KR6 w - - 0 1").unwrap());
    assert!(analysis::Key::new(1, &r, 0, "Stockfish 17.1", &Settings::default()).is_err());
}
#[test]
fn sqlite_smoke_storage_impact() {
    let t = Temp::new();
    let mut s = t.store();
    s.db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    let empty = s.size().unwrap();
    let (key, snapshot) = fixture(&mut s);
    s.cache_analysis(&key, &snapshot, true).unwrap();
    s.save_preferences(&Default::default()).unwrap();
    let p = s.active_profile().unwrap();
    s.create_job(p, Some(key.game), "positions", "smoke", 5)
        .unwrap();
    s.db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    let populated = s.size().unwrap();
    let backup = t.path("smoke-backup.sqlite3");
    s.backup(&backup).unwrap();
    println!(
        "SQLite {}: empty database+sidecars={empty} bytes; one game/analysis/job/settings={populated} bytes; backup={} bytes",
        rusqlite::version(),
        std::fs::metadata(&backup).unwrap().len()
    );
    assert_eq!(s.integrity().unwrap(), "ok");
    assert!(populated < 1024 * 1024);
}
#[test]
fn data_inside_git_repository_is_rejected() {
    let t = Temp::new();
    std::fs::create_dir(t.path(".git")).unwrap();
    assert!(Store::open(&t.path("accidental.sqlite3")).is_err());
    assert!(!t.path("accidental.sqlite3").exists());
}
