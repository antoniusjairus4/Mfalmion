use super::*;
use crate::storage::{Store, sync::HttpCache};
use client::{Response, Transport};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp {
    root: PathBuf,
}
impl Temp {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "mfalmion-sync-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        Self { root }
    }
    fn path(&self) -> PathBuf {
        self.root.join("library.sqlite3")
    }
    fn store(&self) -> Store {
        Store::open(&self.path()).unwrap()
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
type Requests = Arc<Mutex<Vec<(String, Option<String>)>>>;
#[derive(Default)]
struct Mock {
    queue: VecDeque<(String, Result<Response>)>,
    seen: Requests,
}
impl Mock {
    fn push(&mut self, url: String, status: u16, body: String) {
        self.queue.push_back((url, Ok(response(status, body))));
    }
    fn account(&mut self, months: &[&str]) {
        self.push(api::profile_url(NAME).unwrap(), 200, profile_json());
        self.push(api::index_url(NAME).unwrap(), 200, index_json(months));
    }
    fn month(&mut self, key: &str, games: Vec<serde_json::Value>) {
        self.push(
            url(key),
            200,
            serde_json::json!({"games":games}).to_string(),
        );
    }
}
impl Transport for Mock {
    fn get(
        &mut self,
        url: &str,
        cache: &HttpCache,
        _limit: usize,
        cancel: &AtomicBool,
    ) -> Result<Response> {
        client::check(cancel)?;
        api::validate_endpoint(url)?;
        self.seen
            .lock()
            .unwrap()
            .push((url.into(), cache.etag.clone()));
        let (expected, result) = self.queue.pop_front().expect("Unexpected mock request");
        assert_eq!(url, expected);
        result
    }
}
const NAME: &str = "Antonius-Jairus-R";
fn now_test() -> i64 {
    httpdate::parse_http_date("Mon, 16 Nov 2026 00:00:00 GMT")
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
fn response(status: u16, body: String) -> Response {
    Response {
        status,
        headers: BTreeMap::from([("etag".into(), "\"fixture\"".into())]),
        body,
    }
}
fn profile_json() -> String {
    serde_json::json!({"username":NAME,"player_id":12345,"name":"Development Player"}).to_string()
}
fn url(key: &str) -> String {
    format!("{}/games/{key}", api::profile_url(NAME).unwrap())
}
fn index_json(months: &[&str]) -> String {
    serde_json::json!({"archives":months.iter().map(|m|url(m)).collect::<Vec<_>>()} ).to_string()
}
fn game(id: u32, moves: &str) -> serde_json::Value {
    serde_json::json!({"url":format!("https://www.chess.com/game/live/{id}"),"pgn":format!("[White \"{NAME}\"]\n[Black \"Opponent\"]\n[Event \"Game {id}\"]\n\n{moves}"),"white":{"username":NAME,"rating":1500,"result":"agreed"},"black":{"username":"Opponent","rating":1490,"result":"agreed"},"end_time":1794700000_i64,"time_class":"rapid","time_control":"600","rated":true,"rules":"chess"})
}
fn execute(store: &mut Store, mock: Mock, full: bool) -> (Progress, Option<Failure>, Mock) {
    let profile = store.active_profile().unwrap();
    let mut runner = runner::Runner { transport: mock };
    let (p, e) = runner.run(
        store,
        profile,
        full,
        &AtomicBool::new(false),
        now_test(),
        |_| {},
    );
    (p, e, runner.transport)
}
#[test]
fn username_validation() {
    for n in [NAME, "Player_1", "1player2"] {
        assert!(api::username(n).is_ok());
    }
    for n in [
        "",
        "ab",
        "1234",
        "x y",
        "../user",
        "a?b",
        "user/other",
        "_user",
        "user-",
        "usér",
    ] {
        assert!(api::username(n).is_err(), "{n}");
    }
    assert_eq!(api::username("SomePlayer").unwrap(), "someplayer");
}
#[test]
fn public_profile_case_identity_and_malformed() {
    assert_eq!(
        api::profile(&profile_json(), &NAME.to_lowercase())
            .unwrap()
            .player_id,
        Some(12345)
    );
    assert!(api::profile("{}", NAME).is_err());
    assert!(api::profile("{bad", NAME).is_err());
    assert!(api::profile(&profile_json(), "OtherPlayer").is_err());
}
#[test]
fn archive_urls_are_strict_and_account_scoped() {
    for value in [
        "https://evil.test/pub/player/antonius-jairus-r/games/2026/11",
        "http://api.chess.com/pub/player/antonius-jairus-r/games/2026/11",
        "https://api.chess.com:443/pub/player/antonius-jairus-r/games/2026/11",
        "https://api.chess.com/pub/player/other/games/2026/11",
        &url("2026/13"),
        &url("2026/1"),
        &format!("{}?redirect=evil", url("2026/11")),
        &format!("{}/../12", url("2026/11")),
    ] {
        assert!(api::archive_url(value, NAME).is_err(), "{value}");
    }
    assert_eq!(api::archive_url(&url("2026/11"), NAME).unwrap().month, 11);
    assert!(api::archive_url(&url("2026/11").replace("antonius-jairus-r", NAME), NAME).is_ok());
}
#[test]
fn discovery_sorts_deduplicates_without_inventing_months() {
    let months = api::archives(&index_json(&["2026/11", "2020/01", "2026/11"]), NAME).unwrap();
    assert_eq!(
        months.iter().map(api::Month::key).collect::<Vec<_>>(),
        ["2020/01", "2026/11"]
    );
    assert!(api::archives("{\"archives\":[]}", NAME).unwrap().is_empty());
    assert!(api::archives("{}", NAME).is_err());
}
#[test]
fn utc_month_and_previous_january_boundary() {
    let current = api::Month::from_unix(now_test());
    assert_eq!(
        current,
        api::Month {
            year: 2026,
            month: 11
        }
    );
    let jan = api::Month {
        year: 2027,
        month: 1,
    };
    assert!(
        api::Month {
            year: 2026,
            month: 12
        }
        .recent(&jan)
    );
    assert!(
        !api::Month {
            year: 2026,
            month: 11
        }
        .recent(&jan)
    );
}
#[test]
fn game_metadata_and_pgn_validation() {
    let g: api::RemoteGame = serde_json::from_value(game(1, "1.e4 e5 *")).unwrap();
    let record = g.validate(NAME).unwrap();
    assert_eq!(record.moves, ["e4", "e5"]);
    assert_eq!(g.white.rating, Some(1500));
    assert_eq!(g.time_class.as_deref(), Some("rapid"));
    assert!(g.rated.unwrap());
    let mut g = g;
    g.white.username = "Different".into();
    assert!(g.validate(NAME).is_err());
}
#[test]
fn invalid_pgn_variants_and_crossprofile_games_rejected() {
    for bad in ["1.e5 *", "1.e4", "1.e4 *\n1.d4 *"] {
        let g: api::RemoteGame = serde_json::from_value(game(1, bad)).unwrap();
        assert!(g.validate(NAME).is_err());
    }
    let mut g: api::RemoteGame = serde_json::from_value(game(1, "1.e4 *")).unwrap();
    g.rules = Some("chess960".into());
    assert!(g.validate(NAME).is_err());
    g.rules = Some("chess".into());
    assert!(g.validate("AnotherPlayer").is_err());
}
#[test]
fn initial_sync_metadata_persistence_and_offline_reopen() {
    let t = Temp::new();
    let mut s = t.store();
    let mut m = Mock::default();
    m.account(&["2026/10", "2026/11"]);
    m.month("2026/10", vec![game(1, "1.e4 *")]);
    m.month("2026/11", vec![game(2, "1.d4 *")]);
    let (p, e, m) = execute(&mut s, m, false);
    assert!(e.is_none());
    assert_eq!(
        (p.discovered, p.processed, p.imported, p.failed),
        (2, 2, 2, 0)
    );
    assert_eq!(m.seen.lock().unwrap().len(), 4);
    assert_eq!(p.last_success, Some(now_test()));
    let id = s.list_games(p.profile, &Default::default()).unwrap()[0].id;
    assert_eq!(s.load_game(p.profile, id).unwrap().moves.len(), 1);
    assert_eq!(
        s.db.query_row("SELECT white_rating FROM remote_games LIMIT 1", [], |r| r
            .get::<_, u32>(
            0
        ))
        .unwrap(),
        1500
    );
    drop(s);
    let s = t.store();
    assert_eq!(s.sync_info(p.profile).unwrap().imported, 2);
    assert_eq!(
        s.list_games(p.profile, &Default::default()).unwrap().len(),
        2
    );
}
#[test]
fn empty_account_and_empty_months_complete() {
    let t = Temp::new();
    let mut s = t.store();
    let mut m = Mock::default();
    m.account(&[]);
    let (p, e, _) = execute(&mut s, m, false);
    assert!(e.is_none());
    assert_eq!((p.discovered, p.processed), (0, 0));
    let mut m = Mock::default();
    m.account(&["2026/11"]);
    m.month("2026/11", vec![]);
    let (p, e, _) = execute(&mut s, m, false);
    assert!(e.is_none());
    assert!(s.month_done(p.profile, "2026/11").unwrap());
}
#[test]
fn duplicate_external_id_does_not_duplicate_or_overwrite() {
    let t = Temp::new();
    let mut s = t.store();
    let mut m = Mock::default();
    m.account(&["2026/11"]);
    m.month("2026/11", vec![game(1, "1.e4 *"), game(1, "1.d4 *")]);
    let (p, e, _) = execute(&mut s, m, false);
    assert!(e.is_none());
    assert_eq!((p.imported, p.skipped), (1, 1));
    let rows = s.list_games(p.profile, &Default::default()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(s.load_game(p.profile, rows[0].id).unwrap().moves, ["e4"]);
}
#[test]
fn manual_fingerprint_match_preserves_original_and_jobs() {
    let t = Temp::new();
    let mut s = t.store();
    let profile = s.active_profile().unwrap();
    let g: api::RemoteGame = serde_json::from_value(game(1, "1.e4 *")).unwrap();
    let record = g.validate(NAME).unwrap();
    s.insert_game(profile, &record, Some("preserved original annotations"))
        .unwrap();
    let id = s.list_games(profile, &Default::default()).unwrap()[0].id;
    let job = s
        .create_job(profile, Some(id), "test", "existing", 0)
        .unwrap();
    s.transition_job(job, crate::storage::jobs::Status::Running, None)
        .unwrap();
    s.transition_job(job, crate::storage::jobs::Status::Completed, None)
        .unwrap();
    assert!(!s.import_remote(profile, &g, &record).unwrap());
    assert_eq!(
        s.load_game(profile, id).unwrap().original_pgn.as_deref(),
        Some("preserved original annotations")
    );
    assert_eq!(
        s.job(job).unwrap().status,
        crate::storage::jobs::Status::Completed
    );
}
#[test]
fn partial_success_records_failure_and_finishes_month() {
    let t = Temp::new();
    let mut s = t.store();
    let mut m = Mock::default();
    m.account(&["2026/11"]);
    m.month(
        "2026/11",
        vec![game(1, "1.e4 *"), game(2, "1.e5 *"), game(3, "1.d4 *")],
    );
    let (p, e, _) = execute(&mut s, m, false);
    assert!(e.is_none());
    assert_eq!((p.imported, p.failed), (2, 1));
    assert_eq!(p.status, "complete_with_errors");
    assert_eq!(
        s.db.query_row("SELECT count(*) FROM sync_failures", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        1
    );
    assert!(s.month_done(p.profile, "2026/11").unwrap());
}
#[test]
fn incremental_refreshes_current_previous_and_new_months_only() {
    let t = Temp::new();
    let mut s = t.store();
    let mut m = Mock::default();
    m.account(&["2026/08", "2026/10", "2026/11"]);
    for (k, id) in [("2026/08", 1), ("2026/10", 2), ("2026/11", 3)] {
        m.month(k, vec![game(id, "1.e4 *")]);
    }
    assert!(execute(&mut s, m, false).1.is_none());
    let mut m = Mock::default();
    m.account(&["2026/08", "2026/09", "2026/10", "2026/11"]);
    m.month("2026/09", vec![game(4, "1.d4 *")]);
    m.month("2026/10", vec![game(2, "1.e4 *")]);
    m.month("2026/11", vec![game(3, "1.e4 *"), game(5, "1.c4 *")]);
    let (p, e, m) = execute(&mut s, m, false);
    assert!(e.is_none());
    assert_eq!(p.imported, 2);
    assert_eq!(m.seen.lock().unwrap().len(), 5);
    assert!(
        !m.seen
            .lock()
            .unwrap()
            .iter()
            .any(|(u, _)| u == &url("2026/08"))
    );
}
#[test]
fn full_resync_revisits_old_months_without_replacing_games() {
    let t = Temp::new();
    let mut s = t.store();
    for full in [false, true] {
        let mut m = Mock::default();
        m.account(&["2020/01"]);
        m.month("2020/01", vec![game(1, "1.e4 *")]);
        let (p, e, m) = execute(&mut s, m, full);
        assert!(e.is_none());
        assert_eq!(m.seen.lock().unwrap().len(), 3);
        if full {
            assert_eq!((p.imported, p.skipped), (0, 1));
        }
    }
}
#[test]
fn conditional_304_reuses_small_json_and_completed_month() {
    let t = Temp::new();
    let mut s = t.store();
    let mut m = Mock::default();
    m.account(&["2026/11"]);
    m.month("2026/11", vec![game(1, "1.e4 *")]);
    execute(&mut s, m, false);
    let mut m = Mock::default();
    for url in [
        api::profile_url(NAME).unwrap(),
        api::index_url(NAME).unwrap(),
        url("2026/11"),
    ] {
        m.push(url, 304, String::new());
    }
    let (p, e, m) = execute(&mut s, m, false);
    assert!(e.is_none());
    assert_eq!((p.imported, p.skipped), (0, 1));
    assert!(
        m.seen
            .lock()
            .unwrap()
            .iter()
            .all(|(_, etag)| etag.as_deref() == Some("\"fixture\""))
    );
}
#[test]
fn cache_freshness_respected_even_full_resync() {
    let t = Temp::new();
    let mut s = t.store();
    let mut m = Mock::default();
    m.account(&["2026/11"]);
    m.month("2026/11", vec![game(1, "1.e4 *")]);
    for (_, r) in &mut m.queue {
        r.as_mut()
            .unwrap()
            .headers
            .insert("cache-control".into(), "max-age=7200".into());
    }
    execute(&mut s, m, false);
    let (p, e, m) = execute(&mut s, Mock::default(), true);
    assert!(e.is_none());
    assert_eq!(p.skipped, 1);
    assert!(m.seen.lock().unwrap().is_empty());
}
#[test]
fn cancelled_month_retains_inserted_games_and_is_retried() {
    let t = Temp::new();
    let mut s = t.store();
    let profile = s.active_profile().unwrap();
    let cancel = AtomicBool::new(false);
    let mut m = Mock::default();
    m.account(&["2026/10", "2026/11"]);
    m.month("2026/10", vec![game(1, "1.e4 *")]);
    m.month("2026/11", vec![game(2, "1.d4 *"), game(3, "1.c4 *")]);
    let mut r = runner::Runner { transport: m };
    let (p, e) = r.run(&mut s, profile, false, &cancel, now_test(), |p| {
        if p.imported == 2 {
            cancel.store(true, Ordering::Release);
        }
    });
    assert!(e.is_some());
    assert_eq!(p.status, "cancelled");
    assert!(s.month_done(profile, "2026/10").unwrap());
    assert!(!s.month_done(profile, "2026/11").unwrap());
    assert_eq!(s.list_games(profile, &Default::default()).unwrap().len(), 2);
    let mut m = Mock::default();
    m.account(&["2026/10", "2026/11"]);
    m.month("2026/10", vec![game(1, "1.e4 *")]);
    m.month("2026/11", vec![game(2, "1.d4 *"), game(3, "1.c4 *")]);
    let (p, e, _) = execute(&mut s, m, false);
    assert!(e.is_none());
    assert_eq!((p.imported, p.skipped), (1, 2));
}
#[test]
fn interrupted_sync_state_recovers_after_reopen() {
    let t = Temp::new();
    let profile;
    {
        let mut s = t.store();
        profile = s.active_profile().unwrap();
        let mut p = s.sync_info(profile).unwrap();
        p.status = "syncing".into();
        p.imported = 4;
        s.save_sync(&p).unwrap();
    }
    let s = t.store();
    s.recover_sync().unwrap();
    let p = s.sync_info(profile).unwrap();
    assert_eq!(p.status, "interrupted");
    assert_eq!(p.imported, 4);
    assert!(p.error.is_some());
}
#[test]
fn http_status_404_429_5xx_and_persisted_global_retry() {
    for status in [404, 429, 500, 503] {
        let t = Temp::new();
        let mut s = t.store();
        let mut m = Mock::default();
        let mut r = response(status, String::new());
        r.headers.insert("retry-after".into(), "120".into());
        m.queue.push_back((api::profile_url(NAME).unwrap(), Ok(r)));
        let (p, e, _) = execute(&mut s, m, false);
        let error = e.unwrap();
        assert!(error.message.contains(&status.to_string()));
        assert_eq!(p.last_success, None);
        assert_eq!(p.retry_at, now_test() + 120);
        assert_eq!(s.http_cooldown().unwrap(), now_test() + 120);
        let (_, e, m) = execute(&mut s, Mock::default(), true);
        assert!(e.is_some());
        assert!(m.seen.lock().unwrap().is_empty());
    }
}
#[test]
fn offline_transport_timeout_keeps_library_usable() {
    let t = Temp::new();
    let mut s = t.store();
    let mut m = Mock::default();
    m.account(&["2026/11"]);
    m.month("2026/11", vec![game(1, "1.e4 *")]);
    execute(&mut s, m, false);
    let mut m = Mock::default();
    m.queue.push_back((
        api::profile_url(NAME).unwrap(),
        Err(Failure {
            message: "simulated timeout".into(),
            offline: true,
            retry_at: 0,
            permanent: false,
        }),
    ));
    let (p, e, _) = execute(&mut s, m, false);
    assert!(e.unwrap().offline);
    assert_eq!(p.status, "offline");
    assert!(p.last_success.is_some());
    assert_eq!(
        s.list_games(p.profile, &Default::default()).unwrap().len(),
        1
    );
}
#[test]
fn malformed_api_never_marks_month_complete() {
    for body in ["{}", "not-json", "{\"games\":{}}"] {
        let t = Temp::new();
        let mut s = t.store();
        let mut m = Mock::default();
        m.account(&["2026/11"]);
        m.push(url("2026/11"), 200, body.into());
        let (p, e, _) = execute(&mut s, m, false);
        assert!(e.is_some());
        assert!(!s.month_done(p.profile, "2026/11").unwrap());
    }
}
#[test]
fn automatic_scheduler_interval_disable_backoff_no_overlap() {
    let p = Preferences::default();
    assert!(p.enabled);
    assert_eq!(p.interval_minutes, 30);
    let mut s = Schedule::new(1, &p, 0);
    assert!(!s.due(1, &p, 1799, false));
    assert!(s.due(1, &p, 1800, false));
    assert!(!s.due(1, &p, 1800, true));
    assert!(!s.due(2, &p, 1800, false));
    assert!(!s.due(
        1,
        &Preferences {
            enabled: false,
            ..p.clone()
        },
        1800,
        false
    ));
    let e = Failure {
        message: "offline".into(),
        offline: true,
        retry_at: 5000,
        permanent: false,
    };
    s.finish(&p, 1800, Some(&e));
    assert_eq!(s.next, 5000);
    for _ in 0..20 {
        s.finish(
            &p,
            6000,
            Some(&Failure {
                retry_at: 0,
                ..e.clone()
            }),
        );
    }
    assert!(s.next <= 7800);
    s.finish(
        &p,
        6000,
        Some(&Failure {
            permanent: true,
            ..e
        }),
    );
    assert!(!s.due(1, &p, 99999, false));
    assert_eq!(s.next_wake(99999), None);
    s.finish(&p, 9000, None);
    assert_eq!(s.next, 10800);
}
#[test]
fn preferences_persist_validate_and_fallback() {
    let t = Temp::new();
    let s = t.store();
    assert_eq!(s.sync_preferences().unwrap().0, Preferences::default());
    let prefs = Preferences {
        enabled: false,
        interval_minutes: 15,
    };
    s.save_sync_preferences(&prefs).unwrap();
    drop(s);
    let s = t.store();
    assert_eq!(s.sync_preferences().unwrap().0, prefs);
    assert!(
        s.save_sync_preferences(&Preferences {
            interval_minutes: 0,
            ..prefs
        })
        .is_err()
    );
    s.db.execute(
        "UPDATE settings SET value='bad' WHERE key='chesscom_preferences'",
        [],
    )
    .unwrap();
    assert!(s.sync_preferences().unwrap().1.is_some());
}
#[test]
fn profile_isolation_same_external_id_and_case_matching() {
    let t = Temp::new();
    let mut s = t.store();
    let a = s.active_profile().unwrap();
    assert_eq!(s.select_profile(&NAME.to_lowercase()).unwrap(), a);
    let g: api::RemoteGame = serde_json::from_value(game(1, "1.e4 *")).unwrap();
    let record = g.validate(NAME).unwrap();
    s.import_remote(a, &g, &record).unwrap();
    let b = s.select_profile("Opponent").unwrap();
    let record = g.validate("Opponent").unwrap();
    s.import_remote(b, &g, &record).unwrap();
    assert_eq!(s.list_games(a, &Default::default()).unwrap().len(), 1);
    assert_eq!(s.list_games(b, &Default::default()).unwrap().len(), 1);
    s.delete_profile(a, true).unwrap();
    assert_eq!(
        s.db.query_row("SELECT count(*) FROM remote_games", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        1
    );
}
#[test]
fn cache_headers_age_http_date_and_errors() {
    let mut r = response(200, String::new());
    r.headers
        .insert("cache-control".into(), "max-age=120".into());
    r.headers.insert("age".into(), "30".into());
    assert_eq!(client::fresh_until(&r, 1000), 1090);
    r.headers.insert("cache-control".into(), "no-store".into());
    assert_eq!(client::fresh_until(&r, 1000), 1000);
    r.status = 429;
    r.headers
        .insert("retry-after".into(), "Mon, 16 Nov 2026 01:00:00 GMT".into());
    assert_eq!(
        client::response_error(&r, now_test()).retry_at,
        now_test() + 3600
    );
    r.status = 302;
    assert!(
        client::response_error(&r, 1000)
            .message
            .contains("redirect")
    );
}
#[test]
fn sync_worker_rejects_overlap_cancels_and_shuts_down() {
    struct Waiting(Arc<AtomicBool>);
    impl Transport for Waiting {
        fn get(
            &mut self,
            _: &str,
            _: &HttpCache,
            _: usize,
            cancel: &AtomicBool,
        ) -> Result<Response> {
            self.0.store(true, Ordering::Release);
            while !cancel.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(5));
            }
            client::check(cancel)?;
            unreachable!()
        }
    }
    let t = Temp::new();
    let s = t.store();
    let profile = s.active_profile().unwrap();
    drop(s);
    let entered = Arc::new(AtomicBool::new(false));
    let client = worker::Client::at(t.path(), Box::new(Waiting(entered.clone()))).unwrap();
    client.start(profile, false).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !entered.load(Ordering::Acquire) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(entered.load(Ordering::Acquire));
    assert!(client.start(profile, true).is_err());
    assert!(client.info(profile).is_err());
    client.cancel();
    while client.busy.load(Ordering::Acquire) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!client.busy.load(Ordering::Acquire));
    assert_eq!(client.take().unwrap().progress.status, "cancelled");
    client.shutdown();
    while !client.done.load(Ordering::Acquire) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(client.done.load(Ordering::Acquire));
    assert_eq!(t.store().integrity().unwrap(), "ok");
}
fn local_server(reply: String, delay: Duration) -> (String, std::thread::JoinHandle<String>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0; 1024];
        while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = socket.read(&mut buffer).unwrap();
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..n]);
            if bytes.len() > 16384 {
                break;
            }
        }
        std::thread::sleep(delay);
        let _ = socket.write_all(reply.as_bytes());
        String::from_utf8_lossy(&bytes).into_owned()
    });
    (format!("http://{address}/fixture"), handle)
}
#[test]
fn http_wire_user_agent_conditional_headers_and_redirect_refusal() {
    let(url,server)=local_server("HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),Duration::ZERO);
    let http = client::Http::local(Duration::from_secs(2));
    let cache = HttpCache {
        etag: Some("\"saved\"".into()),
        ..Default::default()
    };
    let r = http
        .request(&url, &cache, 1024, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(r.status, 302);
    let request = server.join().unwrap();
    assert!(request.to_lowercase().contains("user-agent: mfalmion/"));
    assert!(request.to_lowercase().contains("if-none-match: \"saved\""));
    assert!(
        client::Http::new()
            .get(&url, &cache, 1024, &AtomicBool::new(false))
            .is_err()
    );
}
#[test]
fn http_real_local_timeout_and_body_limit() {
    let http = client::Http::local(Duration::from_millis(40));
    let (url, server) = local_server(
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".into(),
        Duration::from_millis(180),
    );
    let started = Instant::now();
    assert!(
        http.request(&url, &Default::default(), 1024, &AtomicBool::new(false))
            .unwrap_err()
            .offline
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    server.join().unwrap();
    let (url, server) = local_server(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: 2000\r\nConnection: close\r\n\r\n{}",
            "x".repeat(2000)
        ),
        Duration::ZERO,
    );
    let http = client::Http::local(Duration::from_secs(2));
    assert!(
        http.request(&url, &Default::default(), 100, &AtomicBool::new(false))
            .unwrap_err()
            .message
            .contains("limit")
    );
    server.join().unwrap();
}
#[test]
fn sync_schema_migrates_v1_additively_and_restores_old_backup() {
    use rusqlite::Connection;
    let t = Temp::new();
    let legacy = t.root.join("legacy.sqlite3");
    let db = Connection::open(&legacy).unwrap();
    // Build the actual v1 schema, not a fabricated migration mock.
    db.execute_batch(crate::storage::schema_for_test()).unwrap();
    db.pragma_update(None, "application_id", 0x4d46414c_i64)
        .unwrap();
    db.pragma_update(None, "user_version", 1).unwrap();
    db.execute("INSERT INTO schema_migrations(version) VALUES(1)", [])
        .unwrap();
    db.execute(
        "INSERT INTO profiles(id,username,active) VALUES(1,'Antonius-Jairus-R',1)",
        [],
    )
    .unwrap();
    drop(db);
    let original = std::fs::read(&legacy).unwrap();
    let mut store = t.store();
    store
        .restore(&legacy, &t.root.join("safety.sqlite3"), true)
        .unwrap();
    assert_eq!(store.active_profile().unwrap(), 1);
    assert_eq!(
        store
            .db
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        3
    );
    assert_eq!(store.integrity().unwrap(), "ok");
    assert_eq!(std::fs::read(&legacy).unwrap(), original);
    drop(store);
    let upgraded = Store::open(&legacy).unwrap();
    assert_eq!(upgraded.profiles().unwrap().len(), 1);
    assert_eq!(
        upgraded
            .db
            .query_row("SELECT count(*) FROM schema_migrations", [], |r| r
                .get::<_, u32>(0))
            .unwrap(),
        3
    );
}
#[test]
fn cancelled_full_refresh_marks_previous_checkpoint_pending() {
    let t = Temp::new();
    let mut s = t.store();
    let mut m = Mock::default();
    m.account(&["2020/01"]);
    m.month("2020/01", vec![game(1, "1.e4 *")]);
    execute(&mut s, m, false);
    let profile = s.active_profile().unwrap();
    let cancel = AtomicBool::new(false);
    let mut m = Mock::default();
    m.account(&["2020/01"]);
    m.month("2020/01", vec![game(1, "1.e4 *"), game(2, "1.d4 *")]);
    let mut r = runner::Runner { transport: m };
    r.run(&mut s, profile, true, &cancel, now_test(), |p| {
        if p.skipped == 1 {
            cancel.store(true, Ordering::Release);
        }
    });
    assert!(!s.month_done(profile, "2020/01").unwrap());
    let mut m = Mock::default();
    m.account(&["2020/01"]);
    m.month("2020/01", vec![game(1, "1.e4 *"), game(2, "1.d4 *")]);
    let (p, e, _) = execute(&mut s, m, false);
    assert!(e.is_none());
    assert_eq!(p.imported, 1);
}
#[test]
fn unchanged_partial_month_keeps_error_status_and_counts() {
    let t = Temp::new();
    let mut s = t.store();
    let mut m = Mock::default();
    m.account(&["2026/11"]);
    m.month("2026/11", vec![game(1, "1.e4 *"), game(2, "1.e5 *")]);
    execute(&mut s, m, false);
    let mut m = Mock::default();
    m.account(&["2026/11"]);
    m.push(url("2026/11"), 304, String::new());
    let (p, e, _) = execute(&mut s, m, false);
    assert!(e.is_none());
    assert_eq!(p.failed, 1);
    assert_eq!(p.status, "complete_with_errors");
    assert_eq!(
        s.db.query_row("SELECT status FROM archive_sync", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "complete_with_errors"
    );
}
#[test]
fn sync_preserves_completed_analysis_cache() {
    use crate::engine::{
        Settings,
        protocol::{Bound, Info, Score, Snapshot},
    };
    let t = Temp::new();
    let mut s = t.store();
    let p = s.active_profile().unwrap();
    let g: api::RemoteGame = serde_json::from_value(game(1, "1.e4 *")).unwrap();
    let r = g.validate(NAME).unwrap();
    s.insert_game(p, &r, Some(&g.pgn)).unwrap();
    let id = s.list_games(p, &Default::default()).unwrap()[0].id;
    let settings = Settings {
        depth: 4,
        multipv: 1,
        ..Default::default()
    };
    let key = crate::storage::analysis::Key::new(id, &r, 0, "Stockfish 17.1", &settings).unwrap();
    let snapshot = Snapshot {
        depth: 4,
        lines: vec![Info {
            depth: Some(4),
            seldepth: None,
            multipv: 1,
            nodes: None,
            nps: None,
            time_ms: None,
            score: Some(Score::Cp(20)),
            bound: Bound::Exact,
            pv: vec!["e2e4".into()],
        }],
    };
    s.cache_analysis(&key, &snapshot, true).unwrap();
    assert!(!s.import_remote(p, &g, &r).unwrap());
    assert!(s.cached_analysis(&key).unwrap().is_some());
}
#[test]
fn storage_rejects_wrong_profile_even_for_direct_import() {
    let t = Temp::new();
    let mut s = t.store();
    let p = s.select_profile("Unrelated").unwrap();
    let g: api::RemoteGame = serde_json::from_value(game(1, "1.e4 *")).unwrap();
    let r = g.validate(NAME).unwrap();
    assert!(s.import_remote(p, &g, &r).is_err());
    assert!(s.list_games(p, &Default::default()).unwrap().is_empty());
}
#[test]
fn failed_schema_migration_rolls_back_all_alters() {
    let t = Temp::new();
    let path = t.path();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(crate::storage::schema_for_test()).unwrap();
    db.pragma_update(None, "application_id", 0x4d46414c_i64)
        .unwrap();
    db.pragma_update(None, "user_version", 1).unwrap();
    db.execute("INSERT INTO schema_migrations(version) VALUES(1)", [])
        .unwrap();
    db.execute("CREATE TABLE remote_games(dummy TEXT)", [])
        .unwrap();
    drop(db);
    assert!(Store::open(&path).is_err());
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM pragma_table_info('profiles') WHERE name='chesscom_id'",
            [],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        0
    );
}
#[test]
#[ignore = "Explicit bounded live Chess.com smoke; never part of offline tests"]
fn live_chesscom_smoke() {
    let mut http = client::Http::new();
    let cancel = AtomicBool::new(false);
    let mut get = |url: &str, limit| -> Option<String> {
        match http.get(url, &Default::default(), limit, &cancel) {
            Ok(r) if r.status == 200 => Some(r.body),
            Ok(r) => {
                println!("SMOKE SKIPPED: live API unavailable, HTTP {}", r.status);
                None
            }
            Err(e) => {
                println!("SMOKE SKIPPED: {}", e.message);
                None
            }
        }
    };
    let Some(body) = get(&api::profile_url(NAME).unwrap(), 1024 * 1024) else {
        return;
    };
    let public = api::profile(&body, NAME).expect("Live profile validation");
    println!(
        "Profile verified: {} (ID {:?})",
        public.username, public.player_id
    );
    let Some(body) = get(&api::index_url(NAME).unwrap(), 1024 * 1024) else {
        return;
    };
    let months = api::archives(&body, NAME).expect("Live archive discovery");
    println!("Archive index verified: {} months", months.len());
    let Some(month) = months.last() else {
        println!("SMOKE SKIPPED: no monthly archives");
        return;
    };
    let Some(body) = get(&month.url(NAME).unwrap(), 32 * 1024 * 1024) else {
        return;
    };
    let games = api::monthly(&body).expect("Live monthly JSON");
    let count = games.len();
    let mut accepted = None;
    for value in games {
        if let Ok(game) = serde_json::from_value::<api::RemoteGame>(value)
            && let Ok(record) = game.validate(NAME)
        {
            accepted = Some((game.url, record.moves.len()));
            break;
        }
    }
    let Some((url, plies)) = accepted else {
        println!(
            "SMOKE SKIPPED: archive {} has no supported valid PGN ({} games)",
            month.key(),
            count
        );
        return;
    };
    println!(
        "SMOKE PASSED: {} games in {}; one validated PGN, {} plies, {}. Three requests only; no database writes.",
        count,
        month.key(),
        plies,
        url
    );
}
#[test]
fn distinct_external_ids_are_not_merged_when_pgn_is_identical() {
    let t = Temp::new();
    let mut s = t.store();
    let p = s.active_profile().unwrap();
    let first: api::RemoteGame = serde_json::from_value(game(1, "1.e4 *")).unwrap();
    let mut second: api::RemoteGame = serde_json::from_value(game(2, "1.e4 *")).unwrap();
    second.pgn = first.pgn.clone();
    for g in [&first, &second, &first, &second] {
        let r = g.validate(NAME).unwrap();
        s.import_remote(p, g, &r).unwrap();
    }
    assert_eq!(s.list_games(p, &Default::default()).unwrap().len(), 2);
    assert_eq!(
        s.db.query_row(
            "SELECT count(DISTINCT game_id) FROM remote_games",
            [],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        2
    );
}
#[test]
fn failure_details_are_bounded_and_replaced_on_successful_resync() {
    let t = Temp::new();
    let mut s = t.store();
    let profile = s.active_profile().unwrap();
    for i in 0..110 {
        s.sync_failure(profile, "2026/11", i, "invalid").unwrap();
    }
    assert_eq!(
        s.db.query_row("SELECT count(*) FROM sync_failures", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        100
    );
    let mut m = Mock::default();
    m.account(&["2026/11"]);
    m.month("2026/11", vec![game(1, "1.e4 *")]);
    let (p, e, _) = execute(&mut s, m, true);
    assert!(e.is_none());
    assert_eq!(p.failed, 0);
    assert_eq!(
        s.db.query_row("SELECT count(*) FROM sync_failures", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        0
    );
}
#[test]
fn transient_database_lock_never_completes_a_month() {
    let t = Temp::new();
    let mut s = t.store();
    let profile = s.active_profile().unwrap();
    let other = rusqlite::Connection::open(t.path()).unwrap();
    let held = std::cell::Cell::new(false);
    let mut m = Mock::default();
    m.account(&["2026/11"]);
    m.month("2026/11", vec![game(1, "1.e4 *")]);
    let mut runner = runner::Runner { transport: m };
    let (p, e) = runner.run(
        &mut s,
        profile,
        false,
        &AtomicBool::new(false),
        now_test(),
        |p| {
            if p.discovered == 1 && !held.get() {
                other.execute_batch("BEGIN IMMEDIATE").unwrap();
                held.set(true);
            }
        },
    );
    other.execute_batch("ROLLBACK").unwrap();
    assert!(e.is_some());
    assert_eq!(p.status, "error");
    assert!(!s.month_done(profile, "2026/11").unwrap());
    assert_eq!(
        s.db.query_row("SELECT count(*) FROM sync_failures", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        0
    );
}
#[test]
fn api_opening_metadata_is_retained_without_inventing_eco_codes() {
    let t = Temp::new();
    let mut s = t.store();
    let p = s.active_profile().unwrap();
    let mut value = game(1, "1.e4 *");
    value["eco"] = serde_json::json!("https://www.chess.com/openings/Italian-Game");
    value["opening"] = serde_json::json!("Italian Game");
    let g: api::RemoteGame = serde_json::from_value(value).unwrap();
    let r = g.validate(NAME).unwrap();
    s.import_remote(p, &g, &r).unwrap();
    let row = &s.list_games(p, &Default::default()).unwrap()[0];
    assert_eq!(row.opening, "Italian Game");
    assert_eq!(
        s.db.query_row("SELECT eco FROM games", [], |r| r.get::<_, String>(0))
            .unwrap(),
        ""
    );
    assert_eq!(
        s.db.query_row("SELECT eco_url FROM remote_games", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        g.eco.unwrap()
    );
}
