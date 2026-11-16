//! Offline, test-only research. Never registered as a production analysis algorithm.
use super::{Config, Evaluation, GameRecord, MoveResult, classify, metrics, runner};
use serde::{Deserialize, Serialize};
use shakmaty::Position;
use std::collections::BTreeMap;

const PROPOSAL: &str = "mfalmion-expected-score-v2-proposal-1";
#[derive(Deserialize)]
struct Case {
    id: String,
    theme: String,
    pgn: String,
    origin: String,
    interpretation: String,
    reference_label: Option<String>,
    provisional: bool,
    selected: bool,
}
fn cases() -> Vec<Case> {
    serde_json::from_str(include_str!(
        "../../../../docs/fixtures/milestone-06-3-cases.json"
    ))
    .unwrap()
}
fn record(c: &Case) -> GameRecord {
    crate::notation::parse_pgn(&c.pgn)
        .unwrap_or_else(|e| panic!("{}: {e}", c.id))
        .remove(0)
}
#[derive(Clone, Serialize, Deserialize)]
struct Sample {
    id: String,
    depth: u16,
    config: Config,
    before: Evaluation,
    after: Evaluation,
    elapsed_ms: u128,
}
#[derive(Debug, PartialEq, Eq, Serialize)]
enum Confidence {
    High,
    Moderate,
    Provisional,
    Insufficient,
}
#[derive(Serialize)]
struct Assessment {
    algorithm: &'static str,
    policy_provisional: bool,
    confidence: Confidence,
    engine: MoveResult,
    coaching: &'static str,
    needs_verification: bool,
}
fn valid(c: &Case, s: &Sample) -> bool {
    let r = record(c);
    let ply = r.played_moves.len() - 1;
    s.id == c.id
        && s.config.validate().is_ok()
        && s.config.engine.depth == s.depth
        && s.before
            .validate(&r, ply, s.depth, s.config.engine.multipv)
            .is_ok()
        && s.after
            .validate(&r, ply + 1, s.depth, s.config.engine.multipv)
            .is_ok()
        && s.before
            .lines
            .iter()
            .chain(&s.after.lines)
            .all(|l| l.bound == "exact")
}
fn v1(c: &Case, s: &Sample) -> MoveResult {
    let r = record(c);
    let ply = r.played_moves.len() - 1;
    // Match the production base-search tactical evidence, not invented human labels.
    classify::classify(
        &r,
        ply,
        &s.before,
        &s.after,
        classify::Evidence {
            tactical_opportunity: classify::tactical(&r, ply, &s.before),
            ..Default::default()
        },
    )
}
fn assess(c: &Case, samples: &[Sample]) -> Assessment {
    assert!(!samples.is_empty());
    let latest = samples.iter().max_by_key(|s| s.depth).unwrap();
    let mut engine = v1(c, latest);
    let compatible = samples.iter().all(|s| {
        let mut a = s.config.clone();
        let mut b = latest.config.clone();
        a.engine.depth = 12;
        b.engine.depth = 12;
        a == b && valid(c, s)
    });
    let labels: Vec<_> = samples.iter().map(|s| v1(c, s)).collect();
    let complete = compatible && labels.iter().all(|m| m.loss.is_some());
    let mut depths: Vec<_> = samples.iter().map(|s| s.depth).collect();
    depths.sort_unstable();
    depths.dedup();
    let agreement = complete
        && labels
            .iter()
            .all(|m| m.classification == engine.classification && m.best == engine.best);
    // Research criterion reuses v1's negligible-loss band; not a calibrated probability.
    let stable_loss = complete
        && labels
            .iter()
            .map(|m| m.loss.unwrap())
            .fold(f64::NEG_INFINITY, f64::max)
            - labels
                .iter()
                .map(|m| m.loss.unwrap())
                .fold(f64::INFINITY, f64::min)
            < 0.02;
    let confidence = if !complete {
        Confidence::Insufficient
    } else if !agreement || !stable_loss || depths.len() < 2 {
        Confidence::Provisional
    } else if depths.contains(&12) && depths.contains(&16) && depths.contains(&20) {
        Confidence::High
    } else {
        Confidence::Moderate
    };
    if !complete {
        engine.classification = classify::Category::Unclassified;
        engine.loss = None;
        engine.confidence = "insufficient evidence".into();
    }
    // Advanced labels remain disabled in this proposal: this corpus has no independently
    // annotated exceptional examples. Existing conservative evidence tests remain in force.
    let r = record(c);
    let ply = r.played_moves.len() - 1;
    let tactical = complete && classify::tactical(&r, ply, &latest.before);
    let coaching = if !complete {
        "Insufficient evidence"
    } else if tactical {
        "Concrete PV tactical evidence; practical severity not calibrated"
    } else if engine.loss.is_some_and(|l| l > 0.0) {
        "Engine concession; practical significance unvalidated"
    } else {
        "No measured engine loss; no exceptional-move claim"
    };
    let needs_verification = confidence != Confidence::High;
    Assessment {
        algorithm: PROPOSAL,
        policy_provisional: true,
        confidence,
        engine,
        coaching,
        needs_verification,
    }
}
fn report(samples: &[Sample]) -> serde_json::Value {
    let mut details = Vec::new();
    let mut frequency = BTreeMap::<String, usize>::new();
    let mut transitions = BTreeMap::<String, usize>::new();
    let mut by_phase = BTreeMap::<String, usize>::new();
    let mut category_stability = BTreeMap::<String, [usize; 2]>::new();
    let mut sensitivity = BTreeMap::<String, usize>::new();
    let mut phase_labels = BTreeMap::<String, usize>::new();
    let mut changed = BTreeMap::<String, usize>::new();
    let mut unique = std::collections::BTreeSet::new();
    for sample in samples {
        assert!(
            unique.insert((&sample.id, sample.depth)),
            "Duplicate fixture/depth"
        );
    }
    let mut paired = 0;
    let mut agreement = 0;
    let mut advanced = 0;
    let mut primary_changes = 0;
    for c in cases() {
        let mut group: Vec<_> = samples.iter().filter(|s| s.id == c.id).cloned().collect();
        if group.is_empty() {
            continue;
        }
        group.sort_by_key(|s| s.depth);
        for s in &group {
            assert!(
                valid(&c, s),
                "Invalid recorded sample {} depth {}",
                c.id,
                s.depth
            );
            let m = v1(&c, s);
            primary_changes += usize::from(
                assess(&c, std::slice::from_ref(s)).engine.classification != m.classification,
            );
            *frequency
                .entry(format!("{}: {}", s.depth, m.classification.label()))
                .or_default() += 1;
            *by_phase
                .entry(format!("{}: {}", s.depth, m.phase.label()))
                .or_default() += 1;
            *phase_labels
                .entry(format!(
                    "{}: {}: {}",
                    s.depth,
                    m.phase.label(),
                    m.classification.label()
                ))
                .or_default() += 1;
            advanced += usize::from(matches!(
                m.classification,
                classify::Category::Brilliant | classify::Category::Great
            ));
            if let Some(loss) = m.loss {
                for boundary in [0.06, 0.08, 0.10] {
                    *changed
                        .entry(format!("{}: boundary {boundary}", s.depth))
                        .or_default() += usize::from(
                        matches!(
                            m.classification,
                            classify::Category::Good | classify::Category::Inaccuracy
                        ) && loss >= 0.06
                            && loss < boundary,
                    );
                    *sensitivity
                        .entry(format!("Good/Inaccuracy boundary {boundary}: below"))
                        .or_default() += usize::from(loss < boundary);
                }
            }
        }
        for pair in group.windows(2) {
            let a = v1(&c, &pair[0]);
            let b = v1(&c, &pair[1]);
            paired += 1;
            agreement += usize::from(a.classification == b.classification);
            *transitions
                .entry(format!(
                    "{}→{}: {}→{}",
                    pair[0].depth,
                    pair[1].depth,
                    a.classification.label(),
                    b.classification.label()
                ))
                .or_default() += 1;
            let counts = category_stability
                .entry(a.classification.label().into())
                .or_default();
            counts[0] += usize::from(a.classification == b.classification);
            counts[1] += 1;
        }
        let a = assess(&c, &group);
        details.push(serde_json::json!({"id":c.id,"theme":c.theme,"interpretation":c.interpretation,"assessment":a,
            "depths":group.iter().map(|s| serde_json::json!({"depth":s.depth,"v1":v1(&c,s),"proposal":assess(&c,std::slice::from_ref(s)),"before":s.before,"after":s.after})).collect::<Vec<_>>()}));
    }
    serde_json::json!({"proposal":PROPOSAL,"production_algorithm":super::VERSION,"released":false,
        "reference_label_count":0,"confusion_matrix":null,"confusion_matrix_reason":"No independent authoritative labels; depth transitions are not a reference confusion matrix",
        "adjacent_depth_pairs":paired,"agreeing_pairs":agreement,"per_category_stability_agree_total":category_stability,
        "primary_label_changes_v1_to_proposal":primary_changes,"frequencies":frequency,"phase_counts":by_phase,"phase_classifications":phase_labels,"threshold_primary_changes":changed,"depth_transitions":transitions,"threshold_sensitivity":sensitivity,
        "advanced_candidates":advanced,"false_advanced_candidates":null,"false_candidate_reason":"Cannot estimate false positives without independently labeled examples",
        "fixtures":details})
}
#[test]
fn dataset_is_legal_provisional_and_covers_requested_families() {
    let data = cases();
    assert_eq!(data.len(), 32);
    let mut ids = std::collections::BTreeSet::new();
    for c in &data {
        assert!(ids.insert(&c.id));
        assert!(c.provisional && c.reference_label.is_none());
        assert!(!c.origin.is_empty() && !c.interpretation.is_empty());
        assert!(!record(c).played_moves.is_empty());
    }
    for id in [
        "e4",
        "d4",
        "c4",
        "Nf3",
        "b3",
        "f4",
        "sicilian",
        "french",
        "caro-kann",
        "scandinavian",
        "missed-fork",
        "missed-pin",
        "missed-skewer",
        "mate-now",
        "missed-mate",
        "legal-sacrifice",
        "unsound-offer",
        "only-defense",
        "forced-recapture",
        "endgame-tempo",
        "winning-simplification",
    ] {
        assert!(data.iter().any(|c| c.id == id));
    }
}
#[test]
#[ignore = "Explicit bounded calibration experiment; requires new output path, never accesses SQLite"]
fn stockfish_depth_experiment() {
    use runner::Analyzer;
    use std::io::Write;
    let path = std::env::var_os("MFALMION_CALIBRATION_OUTPUT").expect("Set a new output path");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    let mut engine = runner::UciAnalyzer::new().unwrap();
    let control = std::sync::atomic::AtomicU8::new(0);
    let selected = std::env::var("MFALMION_CALIBRATION_CASE").ok();
    let chosen: Vec<_> = cases()
        .into_iter()
        .filter(|c| c.selected && selected.as_ref().is_none_or(|id| id == &c.id))
        .collect();
    assert!(!chosen.is_empty(), "Unknown or unselected fixture");
    for c in chosen {
        let r = record(&c);
        let ply = r.played_moves.len() - 1;
        for depth in [12, 16, 20] {
            let config = Config {
                engine: crate::engine::Settings {
                    depth,
                    multipv: 3,
                    threads: 1,
                    hash_mib: 64,
                    ..Default::default()
                },
                ..Default::default()
            };
            let now = std::time::Instant::now();
            let before = engine.evaluate(&r, ply, &config, depth, &control).unwrap();
            let after = engine
                .evaluate(&r, ply + 1, &config, depth, &control)
                .unwrap();
            let s = Sample {
                id: c.id.clone(),
                depth,
                config,
                before,
                after,
                elapsed_ms: now.elapsed().as_millis(),
            };
            assert!(valid(&c, &s));
            serde_json::to_writer(&mut file, &s).unwrap();
            writeln!(file).unwrap();
            file.flush().unwrap();
            println!(
                "{} depth {}: {} ({}ms)",
                c.id,
                depth,
                v1(&c, &s).classification.label(),
                s.elapsed_ms
            );
        }
    }
}
fn saved() -> Vec<Sample> {
    include_str!("../../../../docs/fixtures/milestone-06-3-depths.jsonl")
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}
#[test]
fn recorded_evidence_and_offline_report() {
    let samples = saved();
    assert_eq!(samples.len(), 30);
    let value = report(&samples);
    assert_eq!(value["released"], false);
    assert_eq!(value["reference_label_count"], 0);
    assert!(value["confusion_matrix"].is_null());
    assert_eq!(value["advanced_candidates"], 0);
    if let Some(path) = std::env::var_os("MFALMION_CALIBRATION_REPORT") {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        serde_json::to_writer_pretty(file, &value).unwrap();
    }
    println!("{}", serde_json::to_string(&serde_json::json!({"pairs":value["adjacent_depth_pairs"],"agree":value["agreeing_pairs"],"frequencies":value["frequencies"],"transitions":value["depth_transitions"]})).unwrap());
}
fn case(id: &str) -> Case {
    cases().into_iter().find(|c| c.id == id).unwrap()
}
fn mock(c: &Case, depth: u16, cp: i32, next_cp: i32) -> Sample {
    use shakmaty::{CastlingMode, uci::UciMove};
    let r = record(c);
    let ply = r.played_moves.len() - 1;
    let eval = |i: usize, value: i32| {
        super::terminal(&r, i).unwrap_or_else(|| Evaluation {
            depth,
            terminal: false,
            lines: r.positions[i]
                .legal_moves()
                .iter()
                .take(3)
                .enumerate()
                .map(|(n, m)| super::Line {
                    rank: n + 1,
                    value: super::Value::Cp(value),
                    bound: "exact".into(),
                    pv: vec![UciMove::from_move(*m, CastlingMode::Standard).to_string()],
                })
                .collect(),
        })
    };
    Sample {
        id: c.id.clone(),
        depth,
        config: Config {
            engine: crate::engine::Settings {
                depth,
                threads: 1,
                ..Default::default()
            },
            ..Default::default()
        },
        before: eval(ply, cp),
        after: eval(ply + 1, next_cp),
        elapsed_ms: 0,
    }
}
#[test]
fn confidence_requires_distinct_compatible_complete_stable_depths() {
    let c = case("b3");
    let a = mock(&c, 12, 40, -20);
    let b = mock(&c, 16, 40, -20);
    let d = mock(&c, 20, 40, -20);
    assert_eq!(
        assess(&c, std::slice::from_ref(&a)).confidence,
        Confidence::Provisional
    );
    assert_eq!(
        assess(&c, &[a.clone(), a.clone()]).confidence,
        Confidence::Provisional
    );
    assert_eq!(
        assess(&c, &[a.clone(), b.clone()]).confidence,
        Confidence::Moderate
    );
    assert_eq!(
        assess(&c, &[a.clone(), b.clone(), d.clone()]).confidence,
        Confidence::High
    );
    let mut bad = d.clone();
    bad.after.lines.pop();
    assert_eq!(
        assess(&c, &[a.clone(), b.clone(), bad]).confidence,
        Confidence::Insufficient
    );
    let mut bad = d.clone();
    bad.before.lines[0].bound = "lower".into();
    assert_eq!(
        assess(&c, &[a.clone(), b.clone(), bad]).confidence,
        Confidence::Insufficient
    );
    let mut bad = d;
    bad.config.engine.threads = 2;
    assert_eq!(
        assess(&c, &[a, b, bad]).confidence,
        Confidence::Insufficient
    );
}
#[test]
fn depth_instability_is_provisional_not_hidden_by_an_opening_name() {
    let c = case("b3");
    let samples = [
        mock(&c, 12, 40, -20),
        mock(&c, 16, 40, 30),
        mock(&c, 20, 40, 30),
    ];
    assert_eq!(assess(&c, &samples).confidence, Confidence::Provisional);
    let unstable = mock(&c, 12, 0, 200);
    let result = assess(&c, &[unstable]);
    assert_eq!(result.confidence, Confidence::Insufficient);
    assert!(result.engine.loss.is_none());
}
#[test]
fn proposal_keeps_v1_thresholds_and_objective_evidence_without_coaching_overrides() {
    for boundary in [0.02, 0.06, 0.12, 0.25] {
        assert_ne!(
            classify::ordinary(boundary - 1e-12),
            classify::ordinary(boundary)
        );
    }
    let c = case("b3");
    let s = mock(&c, 12, 45, -16);
    let a = assess(&c, std::slice::from_ref(&s));
    assert_eq!(a.engine.classification, classify::Category::Inaccuracy);
    assert_eq!(a.engine.loss, v1(&c, &s).loss);
    assert!(a.coaching.contains("unvalidated"));
    assert!(a.policy_provisional && a.needs_verification);
    assert_ne!(a.algorithm, super::VERSION);
    assert!(
        (metrics::expected(&super::Value::Cp(45), true)
            - metrics::expected(&super::Value::Cp(-16), true)
            - 0.07600323070020171)
            .abs()
            < 1e-14
    );
}
#[test]
fn phase_is_context_not_an_automatic_loss_discount() {
    for id in ["b3", "quiet-defense", "endgame-tempo"] {
        let c = case(id);
        let s = mock(&c, 12, 200, 0);
        let a = assess(&c, std::slice::from_ref(&s));
        assert_eq!(a.engine.phase, v1(&c, &s).phase);
        assert_eq!(a.engine.loss, v1(&c, &s).loss);
        assert_eq!(a.engine.classification, v1(&c, &s).classification);
    }
}
#[test]
fn dataset_forced_capture_and_mate_are_authoritative_not_score_guesses() {
    for id in ["only-defense", "forced-recapture"] {
        let r = record(&case(id));
        assert_eq!(r.positions[0].legal_moves().len(), 1, "{id}");
    }
    let r = record(&case("mate-now"));
    assert!(r.positions.last().unwrap().is_checkmate());
    assert_eq!(
        super::terminal(&r, 1).unwrap().lines[0].value,
        super::Value::Mate {
            white: true,
            moves: 0
        }
    );
    assert_eq!(
        metrics::expected(
            &super::Value::Mate {
                white: false,
                moves: 4
            },
            false
        ),
        1.0
    );
    assert_eq!(
        metrics::cp_loss(
            &super::Value::Mate {
                white: true,
                moves: 2
            },
            &super::Value::Cp(0),
            true
        ),
        None
    );
}
#[test]
fn proposal_never_promotes_unverified_sacrifice_or_forced_move_to_exceptional() {
    for id in [
        "legal-sacrifice",
        "unsound-offer",
        "recapture",
        "forced-recapture",
        "only-defense",
    ] {
        let c = case(id);
        let s = mock(&c, 20, 0, 0);
        let a = assess(&c, &[s]);
        assert!(!matches!(
            a.engine.classification,
            classify::Category::Brilliant | classify::Category::Great
        ));
    }
}
#[test]
fn offline_preview_is_pure_and_cannot_enable_proposal_in_production() {
    let c = case("b3");
    let s = mock(&c, 12, 45, -16);
    let original = serde_json::to_string(&s).unwrap();
    let a = assess(&c, std::slice::from_ref(&s));
    assert_eq!(a.engine.classification, v1(&c, &s).classification);
    assert_eq!(serde_json::to_string(&s).unwrap(), original);
    let production = Config::default();
    let mut proposal = production.clone();
    proposal.algorithm = PROPOSAL.into();
    assert_ne!(production.key(), proposal.key());
    assert!(production.validate().is_ok());
    assert!(proposal.validate().is_err());
}

#[test]
fn recorded_b3_stays_objectively_inferior_but_confidence_is_provisional() {
    let samples: Vec<_> = saved().into_iter().filter(|s| s.id == "b3").collect();
    let c = case("b3");
    assert_eq!(samples.len(), 3);
    for s in &samples {
        let m = v1(&c, s);
        assert_eq!(m.classification, classify::Category::Inaccuracy);
        assert_eq!(m.best.as_deref(), Some("e2e4"));
        assert_eq!(m.ply, 1);
        assert!(m.white);
        assert!(m.loss.unwrap() >= 0.06 && m.loss.unwrap() < 0.12);
    }
    let a = assess(&c, &samples);
    assert_eq!(a.confidence, Confidence::Provisional);
    assert!(a.needs_verification);
    assert!(a.coaching.contains("practical significance unvalidated"));
}
#[test]
fn mate_delay_without_losing_forced_win_is_not_automatically_a_miss() {
    let samples: Vec<_> = saved()
        .into_iter()
        .filter(|s| s.id == "missed-mate")
        .collect();
    let c = case("missed-mate");
    assert_eq!(samples.len(), 3);
    for s in &samples {
        assert!(matches!(
            s.before.lines[0].value,
            super::Value::Mate { white: true, .. }
        ));
        assert!(matches!(
            s.after.lines[0].value,
            super::Value::Mate { white: true, .. }
        ));
        let m = v1(&c, s);
        assert_eq!(m.loss, Some(0.0));
        assert_ne!(m.classification, classify::Category::Miss);
    }
}
