//! Display formatting shared by the pi stats endpoint.
//!
//! Costs arrive as microdollars because that is the only unit the provider
//! spine records without rounding. A pane that printed them raw would show
//! `$0.000000` for every early turn, so the scale of the number decides how
//! many decimals it keeps.

/// Microdollars as a dollar string.
pub fn cost(microdollars: i64) -> String {
    let dollars = microdollars as f64 / 1_000_000.0;
    if dollars == 0.0 {
        "$0".to_owned()
    } else if dollars < 0.01 {
        format!("${dollars:.6}")
    } else {
        format!("${dollars:.4}")
    }
}

/// Microdollars as a dollar string with trailing zeros trimmed.
///
/// For a *grant* rather than a spend. A grant is a round figure someone chose —
/// five dollars — and rendering it as `$5.0000` makes a deliberate round number
/// look like a measurement. Spend keeps its decimals, because there the digits
/// are the point.
pub fn cost_round(microdollars: i64) -> String {
    let formatted = cost(microdollars);
    if !formatted.contains('.') {
        return formatted;
    }
    let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
    trimmed.to_owned()
}

/// The median of a set of latencies, or `None` when nothing has completed.
///
/// The median rather than the mean: one cold start on the first turn drags an
/// average far enough to misrepresent every turn after it.
pub fn median(mut values: Vec<i32>) -> Option<i32> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    Some(values[values.len() / 2])
}
