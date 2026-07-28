//! Cost and latency formatting for the live profile pane.

use systemprompt_web_admin::test_support::{cost, cost_round, median};

/// A round grant should read as the figure someone chose, not as a
/// measurement of it.
#[test]
fn a_grant_loses_its_trailing_zeros() {
    assert_eq!(cost_round(1_000_000), "$1");
    assert_eq!(cost_round(5_000_000), "$5");
    assert_eq!(cost_round(2_500_000), "$2.5");
    assert_eq!(cost_round(0), "$0");
}

/// Trimming must not eat a significant digit.
#[test]
fn a_grant_keeps_digits_that_matter() {
    assert_eq!(cost_round(1_230_000), "$1.23");
    assert_eq!(cost_round(10_000), "$0.01");
}


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
