use super::*;
use super::{
    classify::{Category as C, Evidence},
    runner::Analyzer,
};
use crate::storage::{Store, batch::Selection, jobs::Status};
use shakmaty::{Chess, Position};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU8, AtomicU64, Ordering},
};
struct Database {
    dir: PathBuf,
    store: Store,
}
impl Database {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "mfalmion-analysis-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        let store = Store::open(&dir.join("test.sqlite3")).unwrap();
        Self { dir, store }
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn game(text: &str) -> GameRecord {
    crate::notation::parse_pgn(text).unwrap().remove(0)
}
fn short() -> GameRecord {
    game("1. e4 e5 2. Nf3 Nc6 *")
}
fn config() -> Config {
    Config {
        engine: engine::Settings {
            depth: 2,
            threads: 1,
            hash_mib: 16,
            ..Default::default()
        },
        ..Default::default()
    }
}
fn evaluated(cp: i32, best: &str) -> Evaluation {
    Evaluation {
        depth: 16,
        terminal: false,
        lines: vec![Line {
            rank: 1,
            value: Value::Cp(cp),
            bound: "exact".into(),
            pv: vec![best.into()],
        }],
    }
}
fn classified(a: i32, b: i32, best: &str) -> MoveResult {
    classify::classify(
        &short(),
        0,
        &evaluated(a, best),
        &evaluated(b, "e7e5"),
        Evidence::default(),
    )
}
struct Mock {
    calls: usize,
    interrupt: Option<usize>,
    fail: bool,
}
impl Mock {
    fn new() -> Self {
        Self {
            calls: 0,
            interrupt: None,
            fail: false,
        }
    }
}
impl Analyzer for Mock {
    fn evaluate(
        &mut self,
        r: &GameRecord,
        ply: usize,
        c: &Config,
        depth: u16,
        control: &AtomicU8,
    ) -> Result<Evaluation, String> {
        self.calls += 1;
        if self.fail {
            return Err("Mock failure".into());
        }
        if let Some(e) = terminal(r, ply) {
            return Ok(e);
        }
        let p = &r.positions[ply];
        let mut roots: Vec<_> = p
            .legal_moves()
            .iter()
            .map(|m| {
                shakmaty::uci::UciMove::from_move(*m, shakmaty::CastlingMode::Standard).to_string()
            })
            .collect();
        roots.sort();
        if ply < r.moves.len() {
            let played = uci(r, ply);
            roots.retain(|m| *m != played);
            roots.insert(0, played);
        }
        let e = Evaluation {
            depth,
            terminal: false,
            lines: roots
                .into_iter()
                .take(c.engine.multipv)
                .enumerate()
                .map(|(i, m)| Line {
                    rank: i + 1,
                    value: Value::Cp(0),
                    bound: "exact".into(),
                    pv: vec![m],
                })
                .collect(),
        };
        if self.interrupt == Some(self.calls) {
            control.store(1, Ordering::Release);
        }
        Ok(e)
    }
}
fn inserted(db: &mut Database, r: &GameRecord) -> (i64, i64) {
    let p = db.store.active_profile().unwrap();
    db.store.insert_game(p, r, None).unwrap();
    let id = db
        .store
        .db
        .query_row(
            "SELECT id FROM games WHERE profile_id=?1 AND fingerprint=?2",
            rusqlite::params![p, crate::storage::games::fingerprint(r)],
            |row| row.get(0),
        )
        .unwrap();
    (p, id)
}
fn complete(db: &mut Database, r: &GameRecord, c: &Config) -> (i64, i64, i64) {
    let (p, g) = inserted(db, r);
    let j = db.store.start_batch(p, c, Selection::Selected(g)).unwrap();
    db.store.transition_job(j, Status::Running, None).unwrap();
    let pending = db.store.next_batch_game(j).unwrap().unwrap();
    runner::analyze_game(
        &mut db.store,
        &pending,
        p,
        c,
        true,
        &mut Mock::new(),
        &AtomicU8::new(0),
        |_, _| {},
    )
    .unwrap();
    db.store.batch_progress(j).unwrap();
    db.store.transition_job(j, Status::Completed, None).unwrap();
    (p, g, pending.run)
}
#[test]
fn perspective_conversion() {
    assert!(metrics::expected(&Value::Cp(100), true) > 0.5);
    assert_eq!(
        metrics::expected(&Value::Cp(100), false),
        1.0 - metrics::expected(&Value::Cp(100), true)
    );
}
#[test]
fn centipawn_loss_is_nonnegative() {
    assert_eq!(
        metrics::cp_loss(&Value::Cp(100), &Value::Cp(-100), true),
        Some(200)
    );
    assert_eq!(
        metrics::cp_loss(&Value::Cp(100), &Value::Cp(200), true),
        Some(0)
    );
    assert_eq!(
        metrics::cp_loss(&Value::Cp(100), &Value::Cp(200), false),
        Some(100)
    );
}
#[test]
fn mate_is_not_centipawns() {
    let mate = Value::Mate {
        white: true,
        moves: 3,
    };
    assert_eq!(metrics::expected(&mate, true), 1.0);
    assert_eq!(metrics::expected(&mate, false), 0.0);
    assert_eq!(metrics::cp_loss(&mate, &Value::Cp(1000), true), None);
}
#[test]
fn bound_excludes_classification() {
    let mut e = evaluated(20, "e2e4");
    e.lines[0].bound = "lower".into();
    let m = classify::classify(&short(), 0, &e, &evaluated(0, "e7e5"), Evidence::default());
    assert_eq!(m.classification, C::Unclassified);
    assert_eq!(m.loss, None);
}
#[test]
fn best_detection() {
    assert_eq!(classified(0, 0, "e2e4").classification, C::Best);
}
#[test]
fn excellent_threshold() {
    assert_eq!(classify::ordinary(0.019), C::Excellent);
}
#[test]
fn good_threshold() {
    assert_eq!(classify::ordinary(0.02), C::Good);
    assert_eq!(classify::ordinary(0.059), C::Good);
}
#[test]
fn inaccuracy_threshold() {
    assert_eq!(classify::ordinary(0.06), C::Inaccuracy);
}
#[test]
fn mistake_threshold() {
    assert_eq!(classify::ordinary(0.12), C::Mistake);
}
#[test]
fn blunder_threshold() {
    assert_eq!(classify::ordinary(0.25), C::Blunder);
}
#[test]
fn miss_requires_concrete_evidence() {
    let a = evaluated(400, "d2d4");
    let b = evaluated(-100, "e7e5");
    let ordinary = classify::classify(&short(), 0, &a, &b, Evidence::default());
    assert_eq!(ordinary.classification, C::Blunder);
    let m = classify::classify(
        &short(),
        0,
        &a,
        &b,
        Evidence {
            tactical_opportunity: true,
            ..Default::default()
        },
    );
    assert_eq!(m.classification, C::Miss);
}
#[test]
fn tactical_mate_evidence() {
    let mut e = evaluated(0, "d2d4");
    e.lines[0].value = Value::Mate {
        white: true,
        moves: 3,
    };
    assert!(classify::tactical(&short(), 0, &e));
}
#[test]
fn ordinary_pawn_move_is_not_sacrifice() {
    assert!(!classify::sacrifice(&short(), 0, &evaluated(0, "e7e5")));
}
fn sacrifice_fixture() -> GameRecord {
    game("[SetUp \"1\"]\n[FEN \"6k1/5ppp/8/8/8/8/5PPP/3Q2K1 w - - 0 1\"]\n\n1. Qd8 *")
}
#[test]
fn unaccepted_sacrifice_rejected() {
    let r = sacrifice_fixture();
    assert!(!classify::sacrifice(&r, 0, &evaluated(0, "g8h7")));
}
fn alternatives() -> Evaluation {
    let mut e = evaluated(400, "e2e4");
    for (rank, m) in [(2, "d2d4"), (3, "g1f3")] {
        e.lines.push(Line {
            rank,
            value: Value::Cp(-200),
            bound: "exact".into(),
            pv: vec![m.into()],
        });
    }
    e
}
#[test]
fn brilliant_requires_all_evidence() {
    let (record, e, after) = brilliant_case();
    let result = classify::classify(
        &record,
        0,
        &e,
        &after,
        Evidence {
            verified: true,
            sacrifice: true,
            consequential: true,
            tactical_opportunity: false,
        },
    );
    assert_eq!(result.classification, C::Brilliant);
    let result = classify::classify(
        &short(),
        0,
        &e,
        &evaluated(-400, "e7e5"),
        Evidence {
            verified: true,
            sacrifice: true,
            consequential: true,
            tactical_opportunity: false,
        },
    );
    assert_ne!(result.classification, C::Brilliant);
}
#[test]
fn great_requires_verification() {
    let e = alternatives();
    let result = classify::classify(
        &short(),
        0,
        &e,
        &evaluated(400, "e7e5"),
        Evidence {
            verified: true,
            consequential: true,
            ..Default::default()
        },
    );
    assert_eq!(result.classification, C::Great);
    assert_eq!(
        classify::classify(
            &short(),
            0,
            &e,
            &evaluated(400, "e7e5"),
            Evidence::default()
        )
        .classification,
        C::Best
    );
}
#[test]
fn advanced_categories_require_high_depth() {
    let mut e = alternatives();
    e.depth = 2;
    let m = classify::classify(
        &short(),
        0,
        &e,
        &evaluated(400, "e7e5"),
        Evidence {
            verified: true,
            sacrifice: true,
            consequential: true,
            ..Default::default()
        },
    );
    assert_eq!(m.classification, C::Best);
}
#[test]
fn classification_precedence_is_single_primary() {
    let (record, before, after) = brilliant_case();
    let result = classify::classify(
        &record,
        0,
        &before,
        &after,
        Evidence {
            verified: true,
            sacrifice: true,
            consequential: true,
            tactical_opportunity: true,
        },
    );
    assert_eq!(result.classification, C::Brilliant);
    let s = Summary::new(config(), vec![result]);
    assert_eq!(s.counts.values().sum::<usize>(), 1);
}
#[test]
fn classification_confidence() {
    assert_eq!(classified(0, 0, "e2e4").confidence, "configured depth");
    assert_eq!(classified(0, 400, "e2e4").confidence, "insufficient");
}
#[test]
fn search_improvement_not_negative_loss() {
    let m = classified(0, 400, "d2d4");
    assert_eq!(m.loss, None);
    let m = classified(0, 1, "d2d4");
    assert_eq!(m.loss, Some(0.0));
}
#[test]
fn accuracy_bounds() {
    for cp in [-100000, 0, 100000] {
        let m = classified(0, cp, "d2d4");
        let a = metrics::accuracy([&m].into_iter());
        if let Some(v) = a.white {
            assert!((0.0..=100.0).contains(&v));
        }
    }
}
#[test]
fn white_black_separation() {
    let mut b = classified(0, -200, "d2d4");
    b.white = false;
    let w = classified(0, 0, "e2e4");
    let a = metrics::accuracy([&w, &b].into_iter());
    assert_eq!(a.white, Some(100.0));
    assert!(a.black.unwrap() < 100.0);
    assert_eq!(a.eligible, [1, 1]);
}
#[test]
fn checkmate_terminal() {
    let r = game("1. f3 e5 2. g4 Qh4# 0-1");
    assert!(matches!(
        terminal(&r, 4).unwrap().lines[0].value,
        Value::Mate {
            white: false,
            moves: 0
        }
    ));
}
#[test]
fn stalemate_terminal() {
    let p = crate::notation::parse_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
    let r = GameRecord::new(p);
    assert_eq!(terminal(&r, 0).unwrap().lines[0].value, Value::Draw);
}
#[test]
fn resignation_does_not_fabricate_mate() {
    let r = game("1. e4 e5 1-0");
    assert!(terminal(&r, 2).is_none());
}
#[test]
fn timeout_does_not_fabricate_evaluation() {
    let r = game("[Termination \"time forfeit\"]\n\n1. e4 e5 0-1");
    assert!(terminal(&r, 2).is_none());
}
#[test]
fn empty_game_accuracy_is_none() {
    let a = Summary::new(config(), vec![]).accuracy;
    assert_eq!(a.white, None);
    assert_eq!(a.black, None);
}
#[test]
fn feature_phase_detection() {
    assert_eq!(phase::detect(&Chess::default()), phase::Phase::Opening);
    let p = crate::notation::parse_fen("7k/8/6K1/8/8/8/8/8 w - - 0 1").unwrap();
    assert_eq!(phase::detect(&p), phase::Phase::Endgame);
}
#[test]
fn phase_accuracy() {
    let s = Summary::new(config(), vec![classified(0, 0, "e2e4")]);
    assert_eq!(s.phases["Opening"].white, Some(100.0));
    assert_eq!(s.phases["Endgame"].white, None);
}
#[test]
fn complete_analysis_persistence_and_review_reload() {
    let mut db = Database::new();
    let c = config();
    let (p, g, _) = complete(&mut db, &short(), &c);
    let review = db.store.completed_review(p, g, &c).unwrap().unwrap();
    assert_eq!(review.moves.len(), 4);
    assert_eq!(review.accuracy.white, Some(100.0));
    let reopened = Store::open(&db.dir.join("test.sqlite3")).unwrap();
    assert_eq!(
        reopened.completed_review(p, g, &c).unwrap().unwrap(),
        review
    );
    assert_eq!(reopened.account_statistics(p, &c).unwrap().games, 1);
}
#[test]
fn incomplete_game_cannot_be_completed() {
    let mut db = Database::new();
    let (p, g) = inserted(&mut db, &short());
    let c = config();
    let j = db.store.start_batch(p, &c, Selection::Selected(g)).unwrap();
    let run = db.store.next_batch_game(j).unwrap().unwrap().run;
    db.store.mark_batch_run(run, "running", None).unwrap();
    let summary = Summary::new(c, vec![]);
    assert!(db.store.finish_batch_game(run, &short(), &summary).is_err());
    assert!(
        db.store
            .completed_review(p, g, &config())
            .unwrap()
            .is_none()
    );
}
#[test]
fn version_and_configuration_isolation() {
    let mut db = Database::new();
    let c = config();
    let (p, g, _) = complete(&mut db, &short(), &c);
    let mut other = c.clone();
    other.engine.depth += 1;
    assert!(db.store.completed_review(p, g, &other).unwrap().is_none());
    other = c;
    other.algorithm = "future-v2".into();
    assert!(other.validate().is_err());
    assert!(db.store.completed_review(p, g, &other).unwrap().is_none());
}
#[test]
fn history_context_distinguishes_repetition() {
    let r = game("1. Nf3 Nf6 2. Ng1 Ng8 *");
    assert_eq!(r.positions[0], r.positions[4]);
    assert_ne!(context(&r, 0).unwrap(), context(&r, 4).unwrap());
}
#[test]
fn uci_history_contains_all_moves() {
    let r = short();
    let h = engine::History {
        initial: r.positions[0].clone(),
        moves: r.played_moves.clone(),
    };
    assert!(
        h.command(&r.positions[4])
            .unwrap()
            .ends_with("moves e2e4 e7e5 g1f3 b8c6")
    );
    assert!(h.command(&r.positions[0]).is_err());
}
#[test]
fn custom_start_history_and_castling() {
    let r =
        game("[SetUp \"1\"]\n[FEN \"r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 3 15\"]\n\n15. O-O O-O-O *");
    let command = engine::History {
        initial: r.positions[0].clone(),
        moves: r.played_moves.clone(),
    }
    .command(&r.positions[2])
    .unwrap();
    assert!(command.ends_with("moves e1g1 e8c8"));
    assert!(command.contains("3 15"));
}
#[test]
fn fifty_move_claim_is_not_automatic_draw() {
    let r = GameRecord::new(
        crate::notation::parse_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 100 51").unwrap(),
    );
    assert!(terminal(&r, 0).is_none());
    let r = GameRecord::new(
        crate::notation::parse_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 150 76").unwrap(),
    );
    assert!(terminal(&r, 0).is_some());
}
#[test]
fn fivefold_repetition_terminal() {
    let r = game(
        "1. Nf3 Nf6 2. Ng1 Ng8 3. Nf3 Nf6 4. Ng1 Ng8 5. Nf3 Nf6 6. Ng1 Ng8 7. Nf3 Nf6 8. Ng1 Ng8 *",
    );
    assert!(terminal(&r, 12).is_none());
    assert!(terminal(&r, 16).is_some());
}
#[test]
fn pause_resume_reuses_checkpoints() {
    let mut db = Database::new();
    let r = short();
    let c = config();
    let (p, g) = inserted(&mut db, &r);
    let j = db.store.start_batch(p, &c, Selection::Selected(g)).unwrap();
    db.store.transition_job(j, Status::Running, None).unwrap();
    let pending = db.store.next_batch_game(j).unwrap().unwrap();
    let control = AtomicU8::new(0);
    let mut m = Mock {
        interrupt: Some(3),
        ..Mock::new()
    };
    assert!(
        runner::analyze_game(
            &mut db.store,
            &pending,
            p,
            &c,
            true,
            &mut m,
            &control,
            |_, _| {}
        )
        .is_err()
    );
    db.store.stop_batch(j, false).unwrap();
    assert_eq!(db.store.job(j).unwrap().status, Status::Paused);
    assert!(
        db.store
            .load_batch_position(pending.run, &r, 1, c.engine.depth, &c, false)
            .unwrap()
            .is_some()
    );
    control.store(0, Ordering::Release);
    db.store.transition_job(j, Status::Running, None).unwrap();
    let mut m = Mock::new();
    runner::analyze_game(
        &mut db.store,
        &pending,
        p,
        &c,
        true,
        &mut m,
        &control,
        |_, _| {},
    )
    .unwrap();
    assert_eq!(m.calls, 4);
}
#[test]
fn cancellation_preserves_valid_checkpoints() {
    let mut db = Database::new();
    let c = config();
    let (p, g) = inserted(&mut db, &short());
    let j = db.store.start_batch(p, &c, Selection::Selected(g)).unwrap();
    db.store.stop_batch(j, true).unwrap();
    assert_eq!(db.store.job(j).unwrap().status, Status::Cancelled);
    assert!(db.store.resumable_batch(p).unwrap().is_none());
    assert_eq!(db.store.load_game(p, g).unwrap().moves, short().moves);
}
#[test]
fn crash_recovery_never_runs_automatically() {
    let mut db = Database::new();
    let c = config();
    let (p, g) = inserted(&mut db, &short());
    let j = db.store.start_batch(p, &c, Selection::Selected(g)).unwrap();
    db.store.transition_job(j, Status::Running, None).unwrap();
    let run = db.store.next_batch_game(j).unwrap().unwrap().run;
    db.store.mark_batch_run(run, "running", None).unwrap();
    db.store.recover_jobs().unwrap();
    assert_eq!(db.store.job(j).unwrap().status, Status::Paused);
    assert_eq!(db.store.resumable_batch(p).unwrap().unwrap().0, j);
}
#[test]
fn retry_failed_increments_retry_count() {
    let mut db = Database::new();
    let c = config();
    let (p, g) = inserted(&mut db, &short());
    let j = db.store.start_batch(p, &c, Selection::Selected(g)).unwrap();
    db.store.transition_job(j, Status::Running, None).unwrap();
    let run = db.store.next_batch_game(j).unwrap().unwrap().run;
    db.store
        .mark_batch_run(run, "failed", Some("fixture"))
        .unwrap();
    db.store.batch_progress(j).unwrap();
    db.store
        .transition_job(j, Status::Failed, Some("fixture"))
        .unwrap();
    let retry = db.store.start_batch(p, &c, Selection::RetryFailed).unwrap();
    assert_eq!(db.store.job(retry).unwrap().retries, 1);
}
#[test]
fn profile_isolation() {
    let mut db = Database::new();
    let c = config();
    let (p, g, _) = complete(&mut db, &short(), &c);
    let other = db.store.select_profile("Other-account").unwrap();
    assert!(db.store.completed_review(other, g, &c).unwrap().is_none());
    assert_eq!(db.store.batch_coverage(other, &c).unwrap().total, 0);
    assert!(
        db.store
            .start_batch(other, &c, Selection::Selected(g))
            .is_err()
    );
    assert_eq!(db.store.batch_coverage(p, &c).unwrap().analyzed, 1);
}
#[test]
fn duplicate_active_job_prevention() {
    let mut db = Database::new();
    let c = config();
    let (p, g) = inserted(&mut db, &short());
    db.store.start_batch(p, &c, Selection::Selected(g)).unwrap();
    assert!(db.store.start_batch(p, &c, Selection::Selected(g)).is_err());
    let mut other = c;
    other.engine.depth += 1;
    assert!(
        db.store
            .start_batch(p, &other, Selection::Selected(g))
            .is_err()
    );
}
#[test]
fn multipv_partial_rejected() {
    let r = short();
    let c = config();
    let mut m = Mock::new();
    let mut e = m.evaluate(&r, 0, &c, 2, &AtomicU8::new(0)).unwrap();
    e.lines.pop();
    assert!(e.validate(&r, 0, 2, 3).is_err());
}
#[test]
fn depth_and_identity_cache_separation() {
    let mut db = Database::new();
    let r = short();
    let c = config();
    let (_, _, run) = complete(&mut db, &r, &c);
    assert!(
        db.store
            .load_batch_position(run, &r, 0, 3, &c, true)
            .unwrap()
            .is_none()
    );
    let mut other = c;
    other.identity = "Stockfish future".into();
    let p = db.store.active_profile().unwrap();
    let g = db.store.next_batch_game(1).unwrap();
    assert!(g.is_none());
    let j = db
        .store
        .start_batch(p, &other, Selection::Unanalyzed)
        .unwrap();
    let next = db.store.next_batch_game(j).unwrap().unwrap();
    assert!(
        db.store
            .load_batch_position(next.run, &r, 0, 2, &other, true)
            .unwrap()
            .is_none()
    );
}
#[test]
fn reanalysis_preserves_previous_complete_result() {
    let mut db = Database::new();
    let c = config();
    let (p, g, _) = complete(&mut db, &short(), &c);
    let j = db
        .store
        .start_batch(p, &c, Selection::Reanalyze(g))
        .unwrap();
    let run = db.store.next_batch_game(j).unwrap().unwrap().run;
    assert!(
        db.store
            .load_batch_position(run, &short(), 0, 2, &c, false)
            .unwrap()
            .is_none()
    );
    assert!(db.store.completed_review(p, g, &c).unwrap().is_some());
}
#[test]
fn transaction_rollback_for_empty_selection() {
    let mut db = Database::new();
    let p = db.store.active_profile().unwrap();
    assert!(
        db.store
            .start_batch(p, &config(), Selection::Selected(999))
            .is_err()
    );
    assert!(db.store.resumable_batch(p).unwrap().is_none());
}
#[test]
fn batch_settings_do_not_change_interactive_settings() {
    let db = Database::new();
    let before = db.store.preferences().unwrap().0;
    db.store.save_batch_config(&config()).unwrap();
    assert_eq!(db.store.preferences().unwrap().0, before);
    assert_eq!(db.store.batch_config().unwrap(), config());
}
#[test]
fn illegal_pv_rejected() {
    let mut e = evaluated(0, "e2e5");
    e.depth = 2;
    assert!(e.validate(&short(), 0, 2, 1).is_err());
}
#[test]
fn engine_failure_never_completes_game() {
    let mut db = Database::new();
    let c = config();
    let r = short();
    let (p, g) = inserted(&mut db, &r);
    let j = db.store.start_batch(p, &c, Selection::Selected(g)).unwrap();
    let pending = db.store.next_batch_game(j).unwrap().unwrap();
    assert!(
        runner::analyze_game(
            &mut db.store,
            &pending,
            p,
            &c,
            true,
            &mut Mock {
                fail: true,
                ..Mock::new()
            },
            &AtomicU8::new(0),
            |_, _| {}
        )
        .is_err()
    );
    assert!(db.store.completed_review(p, g, &c).unwrap().is_none());
}
#[test]
#[ignore = "Opt-in shallow Stockfish 17.1 test; temporary database, one two-ply game only"]
fn real_stockfish_batch_smoke() {
    let mut db = Database::new();
    let mut c = config();
    c.engine.multipv = 1;
    let r = game("1. e4 e5 *");
    let (p, g) = inserted(&mut db, &r);
    let j = db.store.start_batch(p, &c, Selection::Selected(g)).unwrap();
    db.store.transition_job(j, Status::Running, None).unwrap();
    let pending = db.store.next_batch_game(j).unwrap().unwrap();
    runner::analyze_game(
        &mut db.store,
        &pending,
        p,
        &c,
        false,
        &mut runner::UciAnalyzer::new().unwrap(),
        &AtomicU8::new(0),
        |_, _| {},
    )
    .unwrap();
    assert_eq!(
        db.store
            .completed_review(p, g, &c)
            .unwrap()
            .unwrap()
            .moves
            .len(),
        2
    );
}

#[test]
fn sound_offered_minor_evidence_requires_acceptance_and_persistent_deficit() {
    let r = game("[SetUp \"1\"]\n[FEN \"4k3/p7/7p/8/8/5N2/P7/3QK3 w - - 0 1\"]\n\n1. Ng5 *");
    let mut e = evaluated(200, "h6g5");
    e.lines[0].pv = vec!["h6g5".into(), "a2a3".into(), "a7a6".into(), "a3a4".into()];
    assert!(classify::sacrifice(&r, 0, &e));
    // Even this concrete offer cannot earn Brilliant without verified soundness and alternatives.
    let mut before = evaluated(200, "f3g5");
    before.depth = 12;
    let m = classify::classify(
        &r,
        0,
        &before,
        &e,
        Evidence {
            sacrifice: true,
            ..Default::default()
        },
    );
    assert_eq!(m.classification, C::Best);
    let mut unsound = e.clone();
    unsound.lines[0].value = Value::Cp(-800);
    let m = classify::classify(
        &r,
        0,
        &before,
        &unsound,
        Evidence {
            verified: true,
            sacrifice: true,
            consequential: true,
            ..Default::default()
        },
    );
    assert_ne!(m.classification, C::Brilliant);
}
#[test]
fn only_legal_move_cannot_be_great_or_brilliant() {
    let r = game("[SetUp \"1\"]\n[FEN \"7k/8/6K1/8/8/8/8/7R b - - 0 1\"]\n\n1... Kg8 *");
    assert_eq!(r.positions[0].legal_moves().len(), 1);
    let m = classify::classify(
        &r,
        0,
        &evaluated(-200, "h8g8"),
        &evaluated(-200, "h1h8"),
        Evidence {
            verified: true,
            sacrifice: true,
            consequential: true,
            tactical_opportunity: true,
        },
    );
    assert_eq!(m.classification, C::Best);
    assert!(m.tags.contains(&"Only move".into()));
}
#[test]
fn new_schema_preserves_game_and_old_cache_tables() {
    let mut db = Database::new();
    let (p, g) = inserted(&mut db, &short());
    db.store.integrity().unwrap();
    assert_eq!(
        db.store
            .db
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(db.store.load_game(p, g).unwrap().moves.len(), 4);
    let count: i64 = db
        .store
        .db
        .query_row("SELECT count(*) FROM analysis", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}
#[cfg(unix)]
fn mock_config(db: &Database) -> Config {
    use std::os::unix::fs::PermissionsExt;
    let path = db.dir.join("batch");
    std::fs::write(&path, include_str!("../../tests/fixtures/mock_uci.py")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut c = config();
    c.identity = "Mock UCI 17.1".into();
    c.engine.path = path.to_str().unwrap().into();
    c.engine.multipv = 1;
    c
}
#[test]
#[cfg(unix)]
fn mock_uci_full_history_and_checkpoint_lifecycle() {
    let mut db = Database::new();
    let c = mock_config(&db);
    let r = game("1. e4 e5 *");
    let (p, g) = inserted(&mut db, &r);
    let j = db.store.start_batch(p, &c, Selection::Selected(g)).unwrap();
    db.store.transition_job(j, Status::Running, None).unwrap();
    let pending = db.store.next_batch_game(j).unwrap().unwrap();
    runner::analyze_game(
        &mut db.store,
        &pending,
        p,
        &c,
        true,
        &mut runner::UciAnalyzer::new().unwrap(),
        &AtomicU8::new(0),
        |_, _| {},
    )
    .unwrap();
    assert!(db.store.completed_review(p, g, &c).unwrap().is_some());
    let log = std::fs::read_to_string(format!("{}.log", c.engine.path)).unwrap();
    assert!(log.contains("moves e2e4 e7e5\n"));
    assert!(log.contains("setoption name Clear Hash\n"));
}
#[test]
#[cfg(unix)]
fn real_worker_bounded_commands_no_automatic_analysis_and_result_loading() {
    let mut db = Database::new();
    let c = mock_config(&db);
    let (p, g) = inserted(&mut db, &game("1. e4 e5 *"));
    db.store.save_batch_config(&c).unwrap();
    let client =
        worker::Client::with_path(db.dir.join("test.sqlite3"), std::sync::Arc::new(|| {})).unwrap();
    client.send(worker::Command::Inspect(p, Some(g))).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while client.snapshot().revision < 1 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(client.snapshot().coverage.total, 1);
    assert!(!db.dir.join("batch.log").exists());
    client
        .send(worker::Command::Start(p, c.clone(), Selection::Selected(g)))
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while client.snapshot().revision < 2 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(client.snapshot().status, "completed");
    assert_eq!(client.snapshot().coverage.analyzed, 1);
    client.send(worker::Command::Inspect(p, Some(g))).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while client.snapshot().revision < 3 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let reviewed = client.snapshot().review.unwrap();
    assert_eq!(reviewed.3.moves.len(), 2);
    assert_eq!(
        reviewed.2,
        crate::storage::games::fingerprint(&game("1. e4 e5 *"))
    );
    client.shutdown();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !client.done.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(client.done.load(Ordering::Acquire));
}
#[test]
fn reanalysis_resume_retains_fresh_search_policy() {
    let mut db = Database::new();
    let c = config();
    let (p, g, _) = complete(&mut db, &short(), &c);
    let j = db
        .store
        .start_batch(p, &c, Selection::Reanalyze(g))
        .unwrap();
    db.store.stop_batch(j, false).unwrap();
    assert!(!db.store.batch_reuse(j).unwrap());
}
#[test]
fn fractional_accuracy_json_roundtrip_validation() {
    for cp in [-400, -113, -31, -17, -1, 0, 1, 17] {
        let s = Summary::new(config(), vec![classified(0, cp, "d2d4")]);
        let json = serde_json::to_string(&s).unwrap();
        let s: Summary = serde_json::from_str(&json).unwrap();
        // Use a one-ply record so validation also checks timeline cardinality.
        s.validate(&game("1. e4 *")).unwrap();
    }
}
#[test]
fn pending_job_recovered_before_first_search() {
    let mut db = Database::new();
    let c = config();
    let (p, g) = inserted(&mut db, &short());
    let j = db.store.start_batch(p, &c, Selection::Selected(g)).unwrap();
    db.store.recover_jobs().unwrap();
    assert_eq!(db.store.job(j).unwrap().status, Status::Paused);
}
#[test]
fn retry_failed_does_not_repeat_later_success() {
    let mut db = Database::new();
    let c = config();
    let r = short();
    let (p, g) = inserted(&mut db, &r);
    let j = db.store.start_batch(p, &c, Selection::Selected(g)).unwrap();
    db.store.transition_job(j, Status::Running, None).unwrap();
    let run = db.store.next_batch_game(j).unwrap().unwrap().run;
    db.store
        .mark_batch_run(run, "failed", Some("fixture"))
        .unwrap();
    db.store.batch_progress(j).unwrap();
    db.store.transition_job(j, Status::Failed, None).unwrap();
    complete(&mut db, &r, &c);
    assert!(db.store.start_batch(p, &c, Selection::RetryFailed).is_err());
}
#[test]
fn phase_classification_counts_are_persisted() {
    let mut db = Database::new();
    let c = config();
    let (p, g, _) = complete(&mut db, &short(), &c);
    let summary = db.store.completed_review(p, g, &c).unwrap().unwrap();
    assert_eq!(summary.phase_counts["Opening"]["Best"], 4);
}
#[test]
fn corrupt_completed_summary_is_reported() {
    let mut db = Database::new();
    let c = config();
    let (p, g, run) = complete(&mut db, &short(), &c);
    db.store
        .db
        .execute("UPDATE batch_runs SET summary='{}' WHERE id=?1", [run])
        .unwrap();
    assert!(db.store.completed_review(p, g, &c).is_err());
}

fn brilliant_case() -> (GameRecord, Evaluation, Evaluation) {
    let r = game("[SetUp \"1\"]\n[FEN \"4k3/p7/7p/8/8/5N2/P7/3QK3 w - - 0 1\"]\n\n1. Ng5 *");
    let mut before = alternatives();
    before.lines[0].pv = vec!["f3g5".into()];
    before.lines[1].pv = vec!["d1d2".into()];
    before.lines[2].pv = vec!["d1d3".into()];
    let mut after = evaluated(400, "h6g5");
    after.lines[0].pv = vec!["h6g5".into(), "a2a3".into(), "a7a6".into(), "a3a4".into()];
    (r, before, after)
}
#[test]
fn fen_only_cache_is_never_batch_history_evidence() {
    let mut db = Database::new();
    let r = short();
    let c = config();
    let (p, g) = inserted(&mut db, &r);
    let key = crate::storage::analysis::Key::new(g, &r, 0, &c.identity, &c.engine).unwrap();
    let lines = ["e2e4", "d2d4", "g1f3"]
        .iter()
        .enumerate()
        .map(|(i, m)| {
            match engine::protocol::parse(
                &format!("info depth 2 multipv {} score cp 0 pv {m}", i + 1),
                shakmaty::Color::White,
            )
            .unwrap()
            {
                engine::protocol::Message::Info(line) => line,
                _ => unreachable!(),
            }
        })
        .collect();
    db.store
        .cache_analysis(&key, &engine::protocol::Snapshot { depth: 2, lines }, true)
        .unwrap();
    let j = db.store.start_batch(p, &c, Selection::Selected(g)).unwrap();
    let run = db.store.next_batch_game(j).unwrap().unwrap().run;
    assert!(
        db.store
            .load_batch_position(run, &r, 0, 2, &c, true)
            .unwrap()
            .is_none()
    );
}

#[test]
fn deleting_game_from_paused_queue_reconciles_job_totals() {
    let mut db = Database::new();
    let c = config();
    let (p, g) = inserted(&mut db, &short());
    let (_, other) = inserted(&mut db, &game("1. d4 d5 *"));
    let job = db.store.start_batch(p, &c, Selection::Unanalyzed).unwrap();
    db.store.stop_batch(job, false).unwrap();
    db.store.delete_game(p, g, true).unwrap();
    db.store.transition_job(job, Status::Running, None).unwrap();
    db.store.batch_progress(job).unwrap();
    assert_eq!(db.store.job(job).unwrap().total, 1);
    let pending = db.store.next_batch_game(job).unwrap().unwrap();
    assert_eq!(pending.game, other);
    runner::analyze_game(
        &mut db.store,
        &pending,
        p,
        &c,
        true,
        &mut Mock::new(),
        &AtomicU8::new(0),
        |_, _| {},
    )
    .unwrap();
    db.store.batch_progress(job).unwrap();
    db.store
        .transition_job(job, Status::Completed, None)
        .unwrap();
}
#[test]
fn account_statistics_only_count_profile_side_and_compatible_versions() {
    let mut db = Database::new();
    let c = config();
    let r = game("[White \"Antonius-Jairus-R\"]\n[Black \"Opponent\"]\n\n1. e4 e5 *");
    let (p, _, _) = complete(&mut db, &r, &c);
    let stats = db.store.account_statistics(p, &c).unwrap();
    assert_eq!(stats.eligible, 1);
    assert_eq!(stats.colors[0].1, 1);
    assert_eq!(stats.colors[1].1, 0);
    assert_eq!(stats.average_accuracy(), Some(100.0));
    assert_eq!(stats.classification_rates()["Best"], 1.0);
    let mut other = c;
    other.engine.depth += 1;
    assert_eq!(db.store.account_statistics(p, &other).unwrap().games, 0);
}

#[test]
fn review_presentation_loads_recorded_settings_accuracy_counts_and_evaluation_read_only() {
    let mut db = Database::new();
    let c = config();
    let r = short();
    let (p, g, _) = complete(&mut db, &r, &c);
    let mut changed = c.clone();
    changed.engine.depth = 20;
    db.store.save_batch_config(&changed).unwrap();
    let changes: i64 = db
        .store
        .db
        .query_row("SELECT total_changes()", [], |r| r.get(0))
        .unwrap();
    let key = crate::storage::review::ReviewKey {
        profile: p,
        game: g,
        fingerprint: crate::storage::games::fingerprint(&r),
        ply: 2,
    };
    let data = db.store.review_data(&key).unwrap();
    let summary = data.summary.as_ref().unwrap();
    assert_eq!(summary.config, c);
    assert_eq!(summary.accuracy.white, Some(100.0));
    assert_eq!(summary.accuracy.black, Some(100.0));
    assert_eq!(summary.counts["Best"], 4);
    assert_eq!(data.move_at(2).unwrap().san, "e5");
    assert_eq!(data.move_at(1).unwrap().san, "e4");
    assert!(data.move_at(0).is_none());
    assert_eq!(data.evaluation.as_ref().unwrap().value, Value::Cp(0));
    assert_eq!(
        changes,
        db.store
            .db
            .query_row("SELECT total_changes()", [], |r| r.get::<_, i64>(0))
            .unwrap()
    );
}
#[test]
fn review_presentation_missing_and_incomplete_states_are_explicit() {
    let mut db = Database::new();
    let r = short();
    let (p, g) = inserted(&mut db, &r);
    let key = crate::storage::review::ReviewKey {
        profile: p,
        game: g,
        fingerprint: crate::storage::games::fingerprint(&r),
        ply: 0,
    };
    let data = db.store.review_data(&key).unwrap();
    assert!(data.summary.is_none());
    assert!(data.status.contains("Not analyzed"));
    db.store
        .start_batch(p, &config(), Selection::Selected(g))
        .unwrap();
    let data = db.store.review_data(&key).unwrap();
    assert!(data.summary.is_none());
    assert!(data.status.contains("incomplete (pending)"));
}
#[test]
fn review_presentation_rejects_wrong_profile_and_stale_game_identity() {
    let mut db = Database::new();
    let c = config();
    let (p, g, _) = complete(&mut db, &short(), &c);
    let mut key = crate::storage::review::ReviewKey {
        profile: p,
        game: g,
        fingerprint: "stale".into(),
        ply: 0,
    };
    assert!(db.store.review_data(&key).unwrap().summary.is_none());
    key.profile = 999;
    assert!(db.store.review_data(&key).is_err());
}
#[test]
fn review_presentation_retains_previous_complete_during_reanalysis() {
    let mut db = Database::new();
    let c = config();
    let r = short();
    let (p, g, _) = complete(&mut db, &r, &c);
    db.store
        .start_batch(p, &c, Selection::Reanalyze(g))
        .unwrap();
    let key = crate::storage::review::ReviewKey {
        profile: p,
        game: g,
        fingerprint: crate::storage::games::fingerprint(&r),
        ply: 1,
    };
    let data = db.store.review_data(&key).unwrap();
    assert!(data.summary.is_some());
    assert!(data.status.contains("prior completed"));
}

#[test]
fn review_presentation_reports_incompatible_saved_algorithm() {
    let mut db = Database::new();
    let r = short();
    let (p, g, run) = complete(&mut db, &r, &config());
    db.store
        .db
        .execute(
            "UPDATE batch_runs SET algorithm='unsupported-v99' WHERE id=?1",
            [run],
        )
        .unwrap();
    let key = crate::storage::review::ReviewKey {
        profile: p,
        game: g,
        fingerprint: crate::storage::games::fingerprint(&r),
        ply: 0,
    };
    let data = db.store.review_data(&key).unwrap();
    assert!(data.summary.is_none());
    assert!(data.status.contains("incompatible"));
}
#[test]
fn asynchronous_review_query_returns_requested_game_and_position_without_starting_engine() {
    let mut db = Database::new();
    let r = short();
    let (p, g, _) = complete(&mut db, &r, &config());
    let client =
        worker::Client::with_path(db.dir.join("test.sqlite3"), std::sync::Arc::new(|| {})).unwrap();
    let key = crate::storage::review::ReviewKey {
        profile: p,
        game: g,
        fingerprint: crate::storage::games::fingerprint(&r),
        ply: 3,
    };
    client.send(worker::Command::Review(key.clone())).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while client.snapshot().revision < 1 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let data = client.snapshot().review_data;
    assert_eq!(data.key, Some(key));
    assert_eq!(data.move_at(3).unwrap().san, "Nf3");
    assert_eq!(client.snapshot().status, "idle");
    assert!(!client.active.load(Ordering::Acquire));
    client.shutdown();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !client.done.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(client.done.load(Ordering::Acquire));
}

#[test]
fn calibration_proposal_cannot_write_or_replace_existing_completed_review() {
    let mut db = Database::new();
    let c = config();
    let (p, g, _) = complete(&mut db, &short(), &c);
    let original = db.store.completed_review(p, g, &c).unwrap().unwrap();
    let writes = db.store.db.total_changes();
    let mut proposed = c.clone();
    proposed.algorithm = "mfalmion-expected-score-v2-proposal-1".into();
    assert!(
        db.store
            .start_batch(p, &proposed, Selection::Selected(g))
            .is_err()
    );
    assert_eq!(db.store.db.total_changes(), writes);
    assert!(
        db.store
            .completed_review(p, g, &proposed)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        db.store.completed_review(p, g, &c).unwrap().unwrap(),
        original
    );
    let key = crate::storage::review::ReviewKey {
        profile: p,
        game: g,
        fingerprint: crate::storage::games::fingerprint(&short()),
        ply: 1,
    };
    let data = db.store.review_data(&key).unwrap();
    assert_eq!(data.summary.unwrap().config.algorithm, VERSION);
}
