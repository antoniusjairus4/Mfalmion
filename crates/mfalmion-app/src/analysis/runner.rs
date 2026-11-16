use super::*;
use crate::storage::{Store, batch::Pending};
use shakmaty::Position;
use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};
pub trait Analyzer {
    fn evaluate(
        &mut self,
        record: &GameRecord,
        ply: usize,
        config: &Config,
        depth: u16,
        control: &AtomicU8,
    ) -> Result<Evaluation, String>;
}
/// One existing UCI worker, reused sequentially; no independent engine implementation.
pub struct UciAnalyzer {
    client: engine::EngineClient,
    id: u64,
}
impl UciAnalyzer {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            client: engine::EngineClient::new(Arc::new(|| {}))?,
            id: 0,
        })
    }
}
impl Drop for UciAnalyzer {
    fn drop(&mut self) {
        self.client.shutdown();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !self.client.is_shutdown() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Analyzer for UciAnalyzer {
    fn evaluate(
        &mut self,
        record: &GameRecord,
        ply: usize,
        config: &Config,
        depth: u16,
        control: &AtomicU8,
    ) -> Result<Evaluation, String> {
        if let Some(e) = terminal(record, ply) {
            return Ok(e);
        }
        self.id = self.id.checked_add(1).ok_or("Request counter exhausted")?;
        let mut settings = config.engine.clone();
        settings.depth = depth;
        self.client.analyze(engine::Request {
            id: self.id,
            session: self.id,
            position: record.positions[ply].clone(),
            settings,
            history: Some(Box::new(engine::History {
                initial: record.positions[0].clone(),
                moves: record.played_moves[..ply].to_vec(),
            })),
        });
        let deadline = Instant::now() + Duration::from_secs(45);
        let mut snapshot = None;
        let mut identity = false;
        let mut complete = false;
        while Instant::now() < deadline {
            if control.load(Ordering::Acquire) != 0 {
                self.client.stop(self.id + 1);
                return Err("Interrupted by user".into());
            }
            for event in self.client.drain() {
                if event.id() != self.id {
                    continue;
                }
                match event {
                    engine::Event::Identity(_, name) => {
                        if name != config.identity {
                            self.client.shutdown();
                            return Err(format!(
                                "Engine identity mismatch: expected {}, received {name}",
                                config.identity
                            ));
                        }
                        identity = true;
                    }
                    engine::Event::Snapshot(_, s) => snapshot = Some(s),
                    engine::Event::BestMove(..) => complete = true,
                    engine::Event::Error(_, e) => return Err(e),
                    _ => {}
                }
            }
            if complete {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        if !complete || !identity {
            return Err("Engine did not complete the requested analysis".into());
        }
        let snapshot = snapshot.ok_or("No completed MultiPV group")?;
        let result = Evaluation {
            depth: snapshot.depth,
            terminal: false,
            lines: snapshot
                .lines
                .into_iter()
                .map(|l| {
                    Ok(Line {
                        rank: l.multipv,
                        value: match l.score.ok_or("Missing score")? {
                            engine::protocol::Score::Cp(v) => Value::Cp(v),
                            engine::protocol::Score::Mate { winner, moves } => Value::Mate {
                                white: winner.is_white(),
                                moves,
                            },
                        },
                        bound: match l.bound {
                            engine::protocol::Bound::Exact => "exact",
                            engine::protocol::Bound::Lower => "lower",
                            engine::protocol::Bound::Upper => "upper",
                        }
                        .into(),
                        pv: l.pv,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?,
        };
        result.validate(record, ply, depth, config.engine.multipv)?;
        Ok(result)
    }
}
// Explicit checkpoint inputs keep storage validation separate from the engine adapter.
#[allow(clippy::too_many_arguments)]
fn evaluation(
    store: &Store,
    pending: &Pending,
    record: &GameRecord,
    ply: usize,
    c: &Config,
    depth: u16,
    reuse: bool,
    analyzer: &mut dyn Analyzer,
    control: &AtomicU8,
) -> Result<Evaluation, String> {
    if control.load(Ordering::Acquire) != 0 {
        return Err("Interrupted by user".into());
    }
    let e = match store.load_batch_position(pending.run, record, ply, depth, c, reuse)? {
        Some(e) => e,
        None => analyzer.evaluate(record, ply, c, depth, control)?,
    };
    store.save_batch_position(pending.run, record, ply, depth, c, &e)?;
    Ok(e)
}
#[allow(clippy::too_many_arguments)]
pub fn analyze_game(
    store: &mut Store,
    pending: &Pending,
    profile: i64,
    c: &Config,
    reuse: bool,
    analyzer: &mut dyn Analyzer,
    control: &AtomicU8,
    mut progress: impl FnMut(usize, usize),
) -> Result<(), String> {
    let record = store.load_game(profile, pending.game)?;
    store.mark_batch_run(pending.run, "running", None)?;
    // Validate actual engine identity before any cached nonterminal evidence is reused.
    // The probe is bounded to depth 1, and discarded; it cannot complete a cached result.
    if let Some(ply) = (0..record.positions.len()).find(|i| terminal(&record, *i).is_none()) {
        analyzer.evaluate(&record, ply, c, 1, control)?;
    }
    let mut before = evaluation(
        store,
        pending,
        &record,
        0,
        c,
        c.engine.depth,
        reuse,
        analyzer,
        control,
    )?;
    let mut moves = Vec::with_capacity(record.moves.len());
    let mut verifications = 0;
    progress(1, record.positions.len());
    for ply in 0..record.moves.len() {
        let after = evaluation(
            store,
            pending,
            &record,
            ply + 1,
            c,
            c.engine.depth,
            reuse,
            analyzer,
            control,
        )?;
        let root = before.lines.first();
        let best = root
            .and_then(|l| l.pv.first())
            .is_some_and(|m| *m == uci(&record, ply));
        let white = record.positions[ply].turn().is_white();
        let gap = root.zip(before.lines.get(1)).map_or(0.0, |(a, b)| {
            metrics::expected(&a.value, white) - metrics::expected(&b.value, white)
        });
        let consequential = phase::material(&record.positions[ply], white) <= -3
            || before
                .lines
                .get(1)
                .is_some_and(|l| metrics::expected(&l.value, white) < 0.35);
        let mut evidence = classify::Evidence {
            tactical_opportunity: classify::tactical(&record, ply, &before),
            ..Default::default()
        };
        let sacrifice = classify::sacrifice(&record, ply, &after);
        if best
            && gap >= 0.15
            && (consequential || sacrifice)
            && verifications < 4
            && c.engine.depth <= 28
            && record.positions[ply].legal_moves().len() > 1
        {
            verifications += 1;
            let depth = (c.engine.depth + 4).max(16);
            // Optional verification failure does not invalidate completed base evidence.
            let high_before = evaluation(
                store, pending, &record, ply, c, depth, reuse, analyzer, control,
            );
            let high_after = evaluation(
                store,
                pending,
                &record,
                ply + 1,
                c,
                depth,
                reuse,
                analyzer,
                control,
            );
            if control.load(Ordering::Acquire) != 0 {
                return Err("Interrupted by user".into());
            }
            if let (Ok(a), Ok(b)) = (high_before, high_after) {
                evidence.verified = true;
                evidence.sacrifice = classify::sacrifice(&record, ply, &b);
                evidence.consequential = consequential || evidence.sacrifice;
                evidence.tactical_opportunity = classify::tactical(&record, ply, &a);
                let m = classify::classify(&record, ply, &a, &b, evidence);
                moves.push(m);
            } else {
                moves.push(classify::classify(&record, ply, &before, &after, evidence));
            }
        } else {
            moves.push(classify::classify(&record, ply, &before, &after, evidence));
        }
        before = after;
        progress(ply + 2, record.positions.len());
    }
    if control.load(Ordering::Acquire) != 0 {
        return Err("Interrupted by user".into());
    }
    store.finish_batch_game(pending.run, &record, &Summary::new(c.clone(), moves))
}
