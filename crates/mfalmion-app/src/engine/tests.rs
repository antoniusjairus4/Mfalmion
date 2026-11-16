use super::{protocol::*, *};
use shakmaty::{Chess, Color, Position};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};
fn info(text: &str, turn: Color) -> Info {
    match parse(text, turn).unwrap() {
        Message::Info(info) => info,
        _ => panic!("Expected info"),
    }
}
fn sample(depth: u16, index: usize) -> Info {
    info(
        &format!("info depth {depth} multipv {index} score cp 25 nodes 123 pv e2e4"),
        Color::White,
    )
}
fn request(id: u64, path: String) -> Request {
    Request {
        id,
        session: 0,
        position: Chess::default(),
        settings: Settings {
            path,
            depth: 6,
            threads: 1,
            hash_mib: 16,
            ..Settings::default()
        },
    }
}
#[test]
fn handshake_options_and_unknown_messages() {
    assert_eq!(parse("uciok\r\n", Color::White).unwrap(), Message::UciOk);
    assert_eq!(parse("readyok", Color::White).unwrap(), Message::ReadyOk);
    assert_eq!(
        parse("id name Stockfish 17.1", Color::White).unwrap(),
        Message::Name("Stockfish 17.1".into())
    );
    assert_eq!(
        parse(
            "option name Hash type spin default 16 min 1 max 1024",
            Color::White
        )
        .unwrap(),
        Message::Option(
            "Hash".into(),
            EngineOption::Spin(SpinOption { min: 1, max: 1024 })
        )
    );
    assert_eq!(
        parse(
            "option name UCI_AnalyseMode type check default false",
            Color::White
        )
        .unwrap(),
        Message::Option("UCI_AnalyseMode".into(), EngineOption::Check)
    );
    assert_eq!(
        parse("info string NUMA configuration", Color::White).unwrap(),
        Message::Unknown
    );
    assert_eq!(
        parse("bestmove 0000", Color::White).unwrap(),
        Message::BestMove(None)
    );
}
#[test]
fn cp_mate_bounds_and_side_normalization() {
    let line = "info depth 16 seldepth 23 multipv 2 score cp 125 lowerbound nodes 123456 nps 42000 time 250 pv e2e4 e7e5";
    let white = info(line, Color::White);
    let black = info(line, Color::Black);
    assert_eq!(white.score, Some(Score::Cp(125)));
    assert_eq!(black.score, Some(Score::Cp(-125)));
    assert_eq!(white.bound, Bound::Lower);
    assert_eq!(black.bound, Bound::Upper);
    assert_eq!(
        (
            white.depth,
            white.seldepth,
            white.multipv,
            white.nodes,
            white.nps,
            white.time_ms
        ),
        (Some(16), Some(23), 2, Some(123456), Some(42000), Some(250))
    );
    assert_eq!(white.pv, ["e2e4", "e7e5"]);
    assert_eq!(
        info("info score cp -20 upperbound", Color::Black).bound,
        Bound::Lower
    );
    for (turn, value, winner) in [
        (Color::White, 3_i32, Color::White),
        (Color::Black, 3, Color::Black),
        (Color::Black, -2, Color::White),
        (Color::White, 0, Color::Black),
    ] {
        assert_eq!(
            info(&format!("info score mate {value}"), turn).score,
            Some(Score::Mate {
                winner,
                moves: value.unsigned_abs()
            })
        );
    }
    assert_eq!(Score::Cp(0).white_fraction(), 0.5);
    assert!(Score::Cp(500).white_fraction() > Score::Cp(100).white_fraction());
    assert!(Score::Cp(i32::MAX).white_fraction() <= 1.0);
    assert_eq!(
        Score::Mate {
            winner: Color::Black,
            moves: 1
        }
        .white_fraction(),
        0.0
    );
    assert!(Score::Cp(25).label(Bound::Upper).starts_with('≤'));
}
#[test]
fn malformed_output_is_rejected_without_panics() {
    for line in [
        "info depth nope",
        "info nodes -3",
        "info score invalid 2",
        "info score cp 2147483648",
        "info multipv 0",
        "bestmove",
        "option name Hash type spin min 20 max 1",
    ] {
        assert!(parse(line, Color::White).is_err(), "{line}");
    }
    assert!(parse("info score cp -2147483648", Color::Black).is_err());
}
#[test]
fn multipv_snapshots_never_mix_depths_or_incomplete_groups() {
    let mut snapshots = Snapshots::new(3, false);
    assert!(snapshots.push(sample(1, 1)).is_none());
    assert!(snapshots.push(sample(2, 2)).is_none());
    assert!(snapshots.push(sample(1, 3)).is_none());
    assert!(snapshots.push(sample(2, 1)).is_none());
    let snapshot = snapshots.push(sample(2, 3)).unwrap();
    assert_eq!(snapshot.depth, 2);
    assert_eq!(snapshot.lines.len(), 3);
    assert!(snapshot.lines.iter().all(|line| line.depth == Some(2)));
    assert!(snapshots.push(sample(2, 1)).is_none()); // same-depth partial refresh cannot reuse old ranks
    assert!(snapshots.push(sample(1, 2)).is_none());
    assert!(snapshots.push(sample(3, 4)).is_none());
}
#[test]
fn pv_decodes_san_special_moves_and_preserves_position() {
    let root = Chess::default();
    let original = crate::notation::fen(&root);
    let moves = ["e2e4", "d7d5", "e4d5"].map(String::from);
    assert_eq!(decode_pv(&root, &moves).text, "1. e4 d5 2. exd5");
    assert_eq!(crate::notation::fen(&root), original);
    let castle = crate::notation::parse_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
    assert_eq!(
        decode_pv(&castle, &["e1g1".into(), "e8c8".into()]).text,
        "1. O-O O-O-O"
    );
    let promotion = crate::notation::parse_fen("7k/P7/8/8/8/8/8/7K w - - 0 12").unwrap();
    assert_eq!(decode_pv(&promotion, &["a7a8q".into()]).text, "12. a8=Q+");
    let black = root
        .clone()
        .play(
            "e2e4"
                .parse::<shakmaty::uci::UciMove>()
                .unwrap()
                .to_move(&root)
                .unwrap(),
        )
        .unwrap();
    assert_eq!(decode_pv(&black, &["e7e5".into()]).text, "1... e5");
    let ep = crate::notation::parse_fen("7k/8/8/3pP3/8/8/8/7K w - d6 0 12").unwrap();
    assert_eq!(decode_pv(&ep, &["e5d6".into()]).text, "12. exd6");
    let broken = decode_pv(&root, &["e2e4".into(), "e7e4".into()]);
    assert_eq!(broken.text, "1. e4");
    assert!(broken.invalid.is_some());
}
#[test]
fn settings_resource_limits_and_latest_request_coalescing() {
    let mut settings = Settings::default();
    assert!(settings.validate().is_ok());
    settings.threads = 99;
    assert!(settings.validate().is_err());
    settings.threads = 1;
    settings.hash_mib = 9999;
    assert!(settings.validate().is_err());
    settings.hash_mib = 64;
    settings.multipv = 4;
    assert!(settings.validate().is_err());
    settings.multipv = 3;
    settings.depth = 0;
    assert!(settings.validate().is_err());
    let mailbox = Mailbox::default();
    for id in 1..=100 {
        mailbox.put(Work::Analyze(request(id, "mock".into())));
    }
    mailbox.put(Work::Stop(99));
    assert!(matches!(
        mailbox.take(),
        Some(Work::Analyze(Request { id: 100, .. }))
    ));
    assert!(mailbox.take().is_none());
}
#[cfg(unix)]
struct Fixture {
    dir: PathBuf,
    path: String,
}
#[cfg(unix)]
impl Fixture {
    fn new(mode: &str) -> Self {
        use std::os::unix::fs::PermissionsExt;
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("mfalmion uci {} {unique}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join(mode);
        fs::write(&path, include_str!("../../tests/fixtures/mock_uci.py")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            dir,
            path: path.to_str().unwrap().into(),
        }
    }
    fn log(&self) -> String {
        fs::read_to_string(format!("{}.log", self.path)).unwrap_or_default()
    }
}
#[cfg(unix)]
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}
fn client() -> EngineClient {
    EngineClient::with_timeouts(worker::Timeouts {
        startup: Duration::from_millis(800),
        ready: Duration::from_millis(800),
        cancel: Duration::from_millis(300),
        search: Duration::from_secs(3),
    })
    .unwrap()
}
fn wait(client: &EngineClient, condition: impl Fn(&Event) -> bool) -> Vec<Event> {
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut events = Vec::new();
    while Instant::now() < deadline {
        let batch = client.drain();
        let found = batch.iter().any(&condition);
        events.extend(batch);
        if found {
            return events;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("Timed out waiting for engine events: {events:?}")
}
fn close(client: &EngineClient) {
    client.shutdown();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !client.is_shutdown() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(client.is_shutdown());
    assert!(client.pid().is_none());
}
#[cfg(unix)]
#[test]
fn process_partial_reads_options_reuse_and_graceful_quit() {
    let fixture = Fixture::new("partial");
    let client = client();
    client.analyze(request(1, fixture.path.clone()));
    let events = wait(&client, |e| matches!(e, Event::BestMove(1, _)));
    assert!(
        !events.iter().any(|e| matches!(e, Event::Error(..))),
        "{events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e,Event::Snapshot(1,s) if s.lines.len()==3))
    );
    let pid = client.pid().unwrap();
    let mut next = request(2, fixture.path.clone());
    next.session = 1;
    client.analyze(next);
    wait(&client, |e| matches!(e, Event::BestMove(2, _)));
    assert_eq!(client.pid(), Some(pid));
    close(&client);
    let log = fixture.log();
    assert_eq!(log.lines().filter(|s| *s == "uci").count(), 1);
    assert_eq!(log.lines().filter(|s| *s == "ucinewgame").count(), 2);
    assert_eq!(log.matches("setoption name Hash").count(), 1);
    assert!(log.lines().any(|s| s == "quit"));
    #[cfg(target_os = "linux")]
    assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
}
#[cfg(unix)]
#[test]
fn cancellation_restarts_latest_search_and_stop_returns_ready() {
    let fixture = Fixture::new("hold");
    let client = client();
    client.analyze(request(1, fixture.path.clone()));
    wait(&client, |e| matches!(e, Event::Snapshot(1, _)));
    let pid = client.pid();
    for id in 2..=100 {
        client.analyze(request(id, fixture.path.clone()));
    }
    wait(&client, |e| matches!(e, Event::Snapshot(100, _)));
    assert_eq!(pid, client.pid());
    client.stop(101);
    let events = wait(&client, |e| matches!(e, Event::Status(101, Status::Ready)));
    assert!(!events.iter().any(|e| matches!(e, Event::BestMove(101, _))));
    close(&client);
    let log = fixture.log();
    assert!(log.contains("stop\n"));
    let mut searching = false;
    for command in log.lines() {
        if command.starts_with("go ") {
            assert!(!searching, "overlapping searches: {log}");
            searching = true;
        }
        if command == "stop" {
            searching = false;
        }
    }
}
#[cfg(unix)]
#[test]
fn failures_timeouts_eof_malformed_and_shutdown_are_recoverable() {
    for (mode, reason) in [
        ("eof", "EOF"),
        ("no-uci", "startup"),
        ("no-ready", "readiness"),
        ("no-hash", "Hash"),
        ("search-eof", "EOF"),
        ("invalid-utf8", "UTF-8"),
        ("oversized", "64 KiB"),
    ] {
        let fixture = Fixture::new(mode);
        let client = client();
        client.analyze(request(1, fixture.path.clone()));
        let events = wait(&client, |e| matches!(e, Event::Error(1, _)));
        assert!(
            events
                .iter()
                .any(|e| matches!(e,Event::Error(_,error) if error.contains(reason))),
            "{mode}: {events:?}"
        );
        close(&client);
    }
    let fixture = Fixture::new("malformed");
    let client = client();
    client.analyze(request(1, fixture.path.clone()));
    wait(&client, |e| matches!(e, Event::BestMove(1, _)));
    close(&client);
}
#[cfg(unix)]
#[test]
fn cancellation_timeout_forces_reap_and_next_request_can_recover() {
    let fixture = Fixture::new("no-bestmove");
    let client = client();
    client.analyze(request(1, fixture.path.clone()));
    wait(&client, |e| matches!(e, Event::Snapshot(1, _)));
    client.stop(2);
    wait(&client, |e| matches!(e, Event::Error(2, _)));
    assert!(client.pid().is_none());
    let healthy = Fixture::new("partial");
    client.analyze(request(3, healthy.path.clone()));
    wait(&client, |e| matches!(e, Event::BestMove(3, _)));
    close(&client);
}
#[cfg(unix)]
#[test]
fn missing_inaccessible_engine_and_forced_shutdown() {
    let client = client();
    client.analyze(request(1, "/does/not/exist/mfalmion-engine".into()));
    wait(&client, |e| matches!(e, Event::Error(1, _)));
    let fixture = Fixture::new("no-quit");
    client.analyze(request(2, fixture.dir.to_str().unwrap().into()));
    wait(&client, |e| matches!(e, Event::Error(2, _)));
    client.analyze(request(3, fixture.path.clone()));
    wait(&client, |e| matches!(e, Event::BestMove(3, _)));
    close(&client);
}
#[cfg(unix)]
#[test]
fn chatty_engine_cannot_defeat_search_watchdog() {
    let fixture = Fixture::new("flood");
    let client = EngineClient::with_timeouts(worker::Timeouts {
        search: Duration::from_millis(300),
        ..worker::Timeouts::default()
    })
    .unwrap();
    client.analyze(request(1, fixture.path.clone()));
    let events = wait(&client, |e| matches!(e, Event::Error(1, _)));
    assert!(
        events
            .iter()
            .any(|e| matches!(e,Event::Error(_,error) if error.contains("watchdog")))
    );
    close(&client);
}

#[cfg(all(unix, target_os = "linux"))]
#[test]
fn dropping_client_during_search_kills_and_reaps_owned_child() {
    let fixture = Fixture::new("hold");
    let client = client();
    client.analyze(request(1, fixture.path.clone()));
    wait(&client, |e| matches!(e, Event::Snapshot(1, _)));
    let pid = client.pid().unwrap();
    drop(client);
    let process = PathBuf::from(format!("/proc/{pid}"));
    let deadline = Instant::now() + Duration::from_secs(3);
    while process.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        !process.exists(),
        "engine child remained after client teardown"
    );
}

#[test]
#[ignore = "Explicit bounded real Stockfish smoke test; not part of offline unit suite"]
fn real_stockfish_smoke() {
    let client = EngineClient::with_timeouts(worker::Timeouts::default()).unwrap();
    let mut req = request(1, "/usr/games/stockfish".into());
    client.analyze(req.clone());
    let first = wait(&client, |e| matches!(e, Event::BestMove(1, _)));
    assert!(
        !first.iter().any(|e| matches!(e, Event::Error(..))),
        "{first:?}"
    );
    assert!(
        first
            .iter()
            .any(|e| matches!(e,Event::Identity(_,name) if name.contains("17.1")))
    );
    assert!(
        first
            .iter()
            .any(|e| matches!(e,Event::Snapshot(_,s) if s.lines.len()==3 && s.depth==6))
    );
    let pid = client.pid().unwrap();
    println!("Starting position: {first:?}; pid={pid}");
    req.id = 2;
    req.position = crate::notation::parse_fen(
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    )
    .unwrap();
    client.analyze(req);
    let second = wait(&client, |e| matches!(e, Event::BestMove(2, _)));
    assert!(
        !second.iter().any(|e| matches!(e, Event::Error(..))),
        "{second:?}"
    );
    assert!(
        second
            .iter()
            .any(|e| matches!(e,Event::Snapshot(_,s) if s.lines.len()==3 && s.depth==6))
    );
    assert_eq!(client.pid(), Some(pid));
    println!("Middlegame: {second:?}");
    close(&client);
    #[cfg(target_os = "linux")]
    assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
    println!("Graceful shutdown and child reaping verified; no owned Stockfish process remains.");
}
