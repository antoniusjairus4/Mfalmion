//! Mfalmion-specific conservative classification rules; never proprietary classifications.
use super::{Evaluation, MoveResult, Value, metrics, phase, uci};
use crate::notation::GameRecord;
use shakmaty::{Position, Role, uci::UciMove};
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Category {
    Brilliant,
    Great,
    Best,
    Excellent,
    Good,
    Inaccuracy,
    Mistake,
    Blunder,
    Miss,
    Unclassified,
}
impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Self::Brilliant => "Brilliant",
            Self::Great => "Great",
            Self::Best => "Best",
            Self::Excellent => "Excellent",
            Self::Good => "Good",
            Self::Inaccuracy => "Inaccuracy",
            Self::Mistake => "Mistake",
            Self::Blunder => "Blunder",
            Self::Miss => "Miss",
            Self::Unclassified => "Insufficient analysis",
        }
    }
}
#[derive(Default)]
pub struct Evidence {
    pub verified: bool,
    pub sacrifice: bool,
    pub consequential: bool,
    pub tactical_opportunity: bool,
}
pub fn ordinary(loss: f64) -> Category {
    if loss < 0.02 {
        Category::Excellent
    } else if loss < 0.06 {
        Category::Good
    } else if loss < 0.12 {
        Category::Inaccuracy
    } else if loss < 0.25 {
        Category::Mistake
    } else {
        Category::Blunder
    }
}
pub fn classify(
    record: &GameRecord,
    ply: usize,
    before: &Evaluation,
    after: &Evaluation,
    evidence: Evidence,
) -> MoveResult {
    let p = &record.positions[ply];
    let next = &record.positions[ply + 1];
    let white = p.turn().is_white();
    let root = before.lines.first();
    let played = uci(record, ply);
    let best = root.and_then(|l| l.pv.first()).cloned();
    let is_best = best.as_ref() == Some(&played);
    let mut out = MoveResult {
        depth: before.depth,
        ply: ply + 1,
        white,
        san: record.moves[ply].clone(),
        phase: phase::detect(p),
        classification: Category::Unclassified,
        confidence: "insufficient".into(),
        tags: vec![],
        loss: None,
        centipawn_loss: None,
        best,
        pv: root.map_or_else(Vec::new, |l| l.pv.clone()),
    };
    if crate::notation::fen(&record.positions[0])
        != crate::notation::fen(&shakmaty::Chess::default())
    {
        out.tags
            .push("Custom start; earlier history unknown".into());
    }
    if next.is_check() {
        out.tags.push("Check".into());
    }
    if next.is_checkmate() {
        out.tags.push("Checkmate".into());
    }
    let forced = p.legal_moves().len() == 1;
    if forced {
        out.tags.push("Only move".into());
    }
    if p.halfmoves() >= 100 || record.positions[..=ply].iter().filter(|q| *q == p).count() >= 3 {
        out.tags
            .push("Claimable draw (not automatically claimed)".into());
    }
    let Some(a) = root else { return out };
    let Some(b) = after.lines.first() else {
        return out;
    };
    if a.bound != "exact" || b.bound != "exact" || before.terminal {
        out.tags.push("Bounded or terminal root".into());
        return out;
    }
    let start = metrics::expected(&a.value, white);
    let end = metrics::expected(&b.value, white);
    let signed = start - end;
    if signed < -0.03 || (is_best && signed.abs() > 0.03) {
        out.tags.push("Search instability".into());
        return out;
    }
    let loss = signed.clamp(0.0, 1.0);
    out.loss = Some(loss);
    out.centipawn_loss = metrics::cp_loss(&a.value, &b.value, white);
    let verified = evidence.verified
        && before.depth >= 16
        && (after.depth >= 16 || after.terminal)
        && before.lines.iter().all(|l| l.bound == "exact");
    out.confidence = if verified {
        "verified"
    } else {
        "configured depth"
    }
    .into();
    if signed < 0.0 {
        out.tags.push("Small search improvement clamped".into());
    }
    let sound_sacrifice = evidence.sacrifice && sacrifice(record, ply, after);
    if sound_sacrifice {
        out.tags.push("Sacrifice".into());
    }
    let gap = before
        .lines
        .get(1)
        .filter(|l| l.bound == "exact")
        .map_or(0.0, |l| start - metrics::expected(&l.value, white));
    let recapture = record.played_moves[ply].is_capture()
        && ply > 0
        && record.played_moves[ply - 1].is_capture()
        && record.played_moves[ply].to() == record.played_moves[ply - 1].to();
    out.classification = if is_best
        && !forced
        && loss < 0.02
        && verified
        && !recapture
        && sound_sacrifice
        && evidence.consequential
        && before.lines.len() >= 3
        && gap >= 0.20
        && start >= 0.6
    {
        Category::Brilliant
    } else if is_best
        && !forced
        && loss < 0.02
        && verified
        && !recapture
        && evidence.consequential
        && gap >= 0.15
    {
        Category::Great
    } else if is_best {
        Category::Best
    } else if evidence.tactical_opportunity && start >= 0.75 && end <= 0.55 && loss >= 0.20 {
        out.tags.push("Tactical opportunity".into());
        Category::Miss
    } else {
        ordinary(loss)
    };
    out
}
/// Evidence must come from a legal concrete principal variation, never the score alone.
pub fn tactical(record: &GameRecord, ply: usize, evaluation: &Evaluation) -> bool {
    let Some(line) = evaluation.lines.first() else {
        return false;
    };
    let p = &record.positions[ply];
    let white = p.turn().is_white();
    if matches!(line.value,Value::Mate{white:w,..} if w==white) {
        return true;
    }
    let initial = phase::material(p, white);
    let mut p = p.clone();
    let mut forcing = false;
    for u in line.pv.iter().take(12) {
        let Ok(u) = u.parse::<UciMove>() else {
            return false;
        };
        let Ok(m) = u.to_move(&p) else { return false };
        forcing |= m.is_capture();
        p.play_unchecked(m);
        forcing |= p.is_check();
    }
    forcing && phase::material(&p, white) - initial >= 3
}
/// Conservative sacrifice evidence: opponent's best line accepts an offered minor/major,
/// and the material deficit persists for at least four legal plies.
pub fn sacrifice(record: &GameRecord, ply: usize, after: &Evaluation) -> bool {
    let m = record.played_moves[ply];
    if m.is_capture() || matches!(m.role(), Role::Pawn | Role::King) {
        return false;
    }
    let p = &record.positions[ply + 1];
    let white = record.positions[ply].turn().is_white();
    let base = phase::material(&record.positions[ply], white);
    let Some(line) = after.lines.first() else {
        return false;
    };
    if line.bound != "exact" || line.pv.len() < 4 {
        return false;
    }
    let mut p = p.clone();
    let mut captured = false;
    for (i, u) in line.pv.iter().take(4).enumerate() {
        let Ok(u) = u.parse::<UciMove>() else {
            return false;
        };
        let Ok(reply) = u.to_move(&p) else {
            return false;
        };
        if i == 0 {
            captured = reply.is_capture() && reply.to() == m.to();
        }
        p.play_unchecked(reply);
    }
    captured && phase::material(&p, white) <= base - 3
}
