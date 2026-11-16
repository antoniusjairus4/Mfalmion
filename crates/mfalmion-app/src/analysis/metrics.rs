use super::{Accuracy, MoveResult, Value};
/// A bounded quality model, not a calibrated probability or official Chess.com metric.
pub fn expected(value: &Value, white: bool) -> f64 {
    let v = match value {
        Value::Cp(cp) => 0.5 + 0.5 * (f64::from(*cp) / 400.0).tanh(),
        Value::Mate { white, .. } => {
            if *white {
                1.0
            } else {
                0.0
            }
        }
        Value::Draw => 0.5,
    };
    if white { v } else { 1.0 - v }
}
pub fn cp_loss(before: &Value, after: &Value, white: bool) -> Option<i64> {
    match (before, after) {
        (Value::Cp(a), Value::Cp(b)) => {
            Some(((i64::from(*a) - i64::from(*b)) * if white { 1 } else { -1 }).max(0))
        }
        _ => None,
    }
}
pub fn accuracy<'a>(moves: impl Iterator<Item = &'a MoveResult>) -> Accuracy {
    let mut result = Accuracy::default();
    let mut sum = [0.0; 2];
    for m in moves {
        let i = usize::from(!m.white);
        if let Some(loss) = m.loss {
            result.eligible[i] += 1;
            sum[i] += 100.0 * (-5.0 * loss.clamp(0.0, 1.0)).exp();
        } else {
            result.excluded[i] += 1;
        }
    }
    result.white = (result.eligible[0] > 0).then(|| sum[0] / result.eligible[0] as f64);
    result.black = (result.eligible[1] > 0).then(|| sum[1] / result.eligible[1] as f64);
    result
}
