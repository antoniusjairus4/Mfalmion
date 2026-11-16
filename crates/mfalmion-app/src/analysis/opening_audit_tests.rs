//! Opening evidence regressions and explicit, non-persistent engine diagnostics.
use super::*;
use crate::engine::protocol::{self, Message, Score};
use shakmaty::Color;
use std::sync::atomic::AtomicU8;
fn record(pgn: &str) -> GameRecord {
    crate::notation::parse_pgn(pgn).unwrap().remove(0)
}
fn sample(cp: i32, pv: &str) -> Evaluation {
    Evaluation {
        depth: 12,
        terminal: false,
        lines: vec![Line {
            rank: 1,
            value: Value::Cp(cp),
            bound: "exact".into(),
            pv: pv.split_whitespace().map(str::to_owned).collect(),
        }],
    }
}
#[test]
fn saved_b3_game4_reproduces_documented_v1_threshold_without_index_shift() {
    let r = record("1. b3 e5 *");
    let before = sample(
        45,
        "e2e4 e7e5 g1f3 g8f6 f3e5 f6e4 d1f3 e4g5 f3e3 g5e6 d2d4 d7d5 b1c3",
    );
    let after = sample(
        -16,
        "e7e5 c1b2 b8c6 c2c4 g8f6 e2e3 d7d5 c4d5 f6d5 g1f3 c8f5 a2a3 e5e4 f3d4 c6d4 b2d4 d8d7",
    );
    before.validate(&r, 0, 12, 1).unwrap();
    after.validate(&r, 1, 12, 1).unwrap();
    let result = classify::classify(&r, 0, &before, &after, Default::default());
    assert_eq!(result.san, "b3");
    assert_eq!(result.ply, 1);
    assert!(result.white);
    assert_eq!(result.best.as_deref(), Some("e2e4"));
    assert_eq!(result.centipawn_loss, Some(61));
    assert!((result.loss.unwrap() - 0.07600323070020171).abs() < 1e-14);
    assert_eq!(result.classification, classify::Category::Inaccuracy);
    assert!(!result.tags.contains(&"Search instability".into()));
}
#[test]
fn black_uci_reply_score_is_converted_once_to_white_perspective() {
    let Message::Info(info) =
        protocol::parse("info depth 12 multipv 1 score cp 16 pv e7e5", Color::Black).unwrap()
    else {
        panic!("Expected info")
    };
    assert_eq!(info.score, Some(Score::Cp(-16)));
    let white = metrics::expected(&Value::Cp(-16), true);
    let black = metrics::expected(&Value::Cp(-16), false);
    assert!(white < 0.5 && black > 0.5);
    assert!((white + black - 1.0).abs() < 1e-15);
}
#[test]
fn expected_score_matches_independent_logistic_identity_and_fixed_numeric_oracle() {
    assert!((metrics::expected(&Value::Cp(45), true) - 0.5560138905446199).abs() < 1e-14);
    assert!((metrics::expected(&Value::Cp(-16), true) - 0.4800106598444182).abs() < 1e-14);
    for cp in [-800, -400, -45, -16, 0, 16, 45, 400, 800] {
        let logistic = 1.0 / (1.0 + (-f64::from(cp) / 200.0).exp());
        assert!((metrics::expected(&Value::Cp(cp), true) - logistic).abs() < 1e-14);
    }
}
#[test]
fn nonbest_opening_is_not_exempt_and_low_loss_need_not_be_inaccuracy() {
    let r = record("1. b3 *");
    let before = sample(30, "e2e4");
    let after = sample(20, "e7e5");
    let m = classify::classify(&r, 0, &before, &after, Default::default());
    assert_eq!(m.classification, classify::Category::Excellent);
    assert_eq!(classify::ordinary(0.06), classify::Category::Inaccuracy);
    assert_eq!(classify::ordinary(0.06 - 1e-12), classify::Category::Good);
}
#[test]
fn instability_guard_does_not_claim_to_detect_all_positive_search_drift() {
    let r = record("1. b3 *");
    let m = classify::classify(
        &r,
        0,
        &sample(45, "e2e4"),
        &sample(-16, "e7e5"),
        Default::default(),
    );
    assert_eq!(m.classification, classify::Category::Inaccuracy);
    let m = classify::classify(
        &r,
        0,
        &sample(0, "e2e4"),
        &sample(100, "e7e5"),
        Default::default(),
    );
    assert_eq!(m.classification, classify::Category::Unclassified);
    assert!(m.tags.contains(&"Search instability".into()));
}
#[test]
#[ignore = "Explicit bounded Stockfish opening audit; results only in an explicitly supplied temporary JSON file"]
fn deeper_stockfish_opening_audit() {
    use super::runner::Analyzer;
    let output = std::env::var_os("MFALMION_OPENING_AUDIT_OUTPUT")
        .expect("Set an explicit temporary output path");
    let output = std::path::PathBuf::from(output);
    assert!(!output.exists(), "Refusing to overwrite diagnostic results");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)
        .unwrap();
    let c = Config {
        engine: crate::engine::Settings {
            depth: 20,
            ..Default::default()
        },
        ..Default::default()
    };
    let control = AtomicU8::new(0);
    let mut engine = runner::UciAnalyzer::new().unwrap();
    let mut results = Vec::new();
    let cases = [
        ("start", 20, "*", 0),
        ("b3", 20, "1. b3 *", 1),
        ("e4", 20, "1. e4 *", 1),
        ("d4", 20, "1. d4 *", 1),
        ("Nf3", 20, "1. Nf3 *", 1),
        ("c4", 20, "1. c4 *", 1),
        ("b4", 20, "1. b4 *", 1),
        ("f4", 20, "1. f4 *", 1),
        ("e3", 20, "1. e3 *", 1),
        ("g3", 20, "1. g3 *", 1),
    ];
    for (name, depth, pgn, ply) in cases {
        let r = record(pgn);
        let now = std::time::Instant::now();
        let e = engine.evaluate(&r, ply, &c, depth, &control);
        let result = serde_json::json!({"position":name,"requested_depth":depth,"settings":c,"history":engine::History{initial:r.positions[0].clone(),moves:r.played_moves[..ply].to_vec()}.command(&r.positions[ply]).unwrap(),"elapsed_ms":now.elapsed().as_millis(),"evaluation":e.as_ref().ok(),"error":e.as_ref().err()});
        println!("{}", serde_json::to_string(&result).unwrap());
        results.push(result);
        use std::io::{Seek, Write};
        file.rewind().unwrap();
        file.set_len(0).unwrap();
        serde_json::to_writer_pretty(&mut file, &results).unwrap();
        file.flush().unwrap();
        if name == "start" || name == "b3" {
            e.expect("Required comparison must finish at requested depth");
        }
    }
}

#[test]
fn saved_first_move_audit_recomputes_all_labels_and_quantifies_threshold_sensitivity() {
    let csv = include_str!("../../../../docs/fixtures/milestone-06-2-first-moves.csv");
    let mut total = 0;
    let mut b3 = 0;
    let mut below_eight = 0;
    let mut below_ten = 0;
    for row in csv.lines().skip(1) {
        let f: Vec<_> = row.split(',').collect();
        assert_eq!(f.len(), 13);
        let r = record(&format!("1. {} *", f[2]));
        let before = sample(f[6].parse().unwrap(), f[10]);
        let after = sample(f[7].parse().unwrap(), "a7a6");
        let m = classify::classify(&r, 0, &before, &after, Default::default());
        assert_eq!(m.classification.label(), f[9], "game {}", f[0]);
        assert_eq!(m.ply, f[3].parse::<usize>().unwrap());
        assert_eq!(m.san, f[2]);
        assert!(m.white);
        let saved: f64 = f[8].parse().unwrap();
        assert!((m.loss.unwrap() - saved).abs() < 1e-12);
        if m.san == "b3" {
            b3 += 1;
            below_eight += usize::from(saved < 0.08);
            below_ten += usize::from(saved < 0.10);
        }
        total += 1;
    }
    assert_eq!(total, 151);
    assert_eq!((b3, below_eight, below_ten), (78, 28, 72));
}
