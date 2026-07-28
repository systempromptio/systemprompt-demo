//! Rounding and suppression for the signed-in tier of the pulse: buckets must
//! never report traffic as silence, and a sparse window is withheld outright.

use systemprompt_web_admin::test_support::{
    MIN_PEOPLE, bucket, bucket_tokens, window_is_publishable,
};

#[test]
fn small_counts_collapse_to_a_threshold() {
    assert_eq!(bucket(0), "0");
    assert_eq!(bucket(1), "<10");
    assert_eq!(bucket(9), "<10");
}

#[test]
fn rounding_never_reports_traffic_as_silence() {
    assert_eq!(bucket(12), "10");
    assert_eq!(bucket(14), "10");
    assert_eq!(bucket(16), "20");
    assert_eq!(bucket(110), "100");
    assert_eq!(bucket(126), "150");
}

#[test]
fn large_counts_become_short_strings() {
    assert_eq!(bucket(1_240), "1.2k");
    assert_eq!(bucket(2_500_000), "2.5M");
}

#[test]
fn token_counts_use_their_own_scale() {
    assert_eq!(bucket_tokens(400), "<1k");
    assert_eq!(bucket_tokens(48_000), "48k");
    assert_eq!(bucket_tokens(3_400_000), "3.4M");
}

#[test]
fn a_sparse_window_is_withheld() {
    assert!(!window_is_publishable(0));
    assert!(!window_is_publishable(MIN_PEOPLE - 1));
    assert!(window_is_publishable(MIN_PEOPLE));
}
