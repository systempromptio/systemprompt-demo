//! Display formatting shared by the pi stats endpoint.
//!
//! Costs arrive as microdollars because that is the only unit the provider
//! spine records without rounding. A pane that printed them raw would show
//! `$0.000000` for every early turn, so the scale of the number decides how
//! many decimals it keeps.

/// Microdollars as a dollar string.
pub(super) fn cost(microdollars: i64) -> String {
    let dollars = microdollars as f64 / 1_000_000.0;
    if dollars == 0.0 {
        "$0".to_owned()
    } else if dollars < 0.01 {
        format!("${dollars:.6}")
    } else {
        format!("${dollars:.4}")
    }
}

/// The median of a set of latencies, or `None` when nothing has completed.
///
/// The median rather than the mean: one cold start on the first turn drags an
/// average far enough to misrepresent every turn after it.
pub(super) fn median(mut values: Vec<i32>) -> Option<i32> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    Some(values[values.len() / 2])
}

#[cfg(test)]
mod tests {
    use super::{cost, median};

    #[test]
    fn cost_keeps_enough_decimals_to_be_non_zero() {
        assert_eq!(cost(0), "$0");
        assert_eq!(cost(1_200), "$0.001200");
        assert_eq!(cost(50_000), "$0.0500");
    }

    #[test]
    fn median_picks_the_upper_middle_of_an_even_set() {
        assert_eq!(median(vec![]), None);
        assert_eq!(median(vec![30, 10, 20]), Some(20));
        assert_eq!(median(vec![40, 10, 30, 20]), Some(30));
    }
}
