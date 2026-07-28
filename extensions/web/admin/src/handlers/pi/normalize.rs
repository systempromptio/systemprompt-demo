//! Rounding and suppression for the signed-in tier of the pulse.
//!
//! A signed-in visitor is shown the deployment's activity to prove the
//! machinery is not staged for them. That argument needs the *order of
//! magnitude*, not the digits — "about 1.2k requests" makes the point as well
//! as `1,247` does, and `1,247` does something the rounded figure does not: it
//! changes between two polls in a way that is attributable. Watch a counter
//! that moves by one while you know only one other person is on the site and
//! the aggregate has quietly become an observation of that person.
//!
//! So the member tier gets buckets, and the admin tier — which is already
//! entitled to per-user rows — gets the integers. The two are computed from the
//! same query; only the rendering differs.
//!
//! # Suppression
//!
//! Rounding alone does not save a window with two people in it: whatever bucket
//! is shown, a visitor who knows they are one of the two learns the other's
//! usage by subtracting their own. Below [`MIN_PEOPLE`] distinct accounts the
//! whole window is withheld and only lifetime totals remain.
//!
//! Suppression is server-side so the sparse window never leaves the process
//! — hiding it in the browser would put a privacy control in the hands of
//! the party it protects against.

pub const MIN_PEOPLE: i64 = 3;

pub fn bucket(n: i64) -> String {
    match n {
        i64::MIN..=0 => "0".to_owned(),
        1..10 => "<10".to_owned(),
        10..100 => round_to(n, 10).to_string(),
        100..1_000 => round_to(n, 50).to_string(),
        1_000..1_000_000 => format!("{:.1}k", n as f64 / 1_000.0),
        _ => format!("{:.1}M", n as f64 / 1_000_000.0),
    }
}

fn round_to(n: i64, step: i64) -> i64 {
    let rounded = (n + step / 2) / step * step;
    rounded.max(step)
}

pub fn bucket_tokens(n: i64) -> String {
    match n {
        i64::MIN..1_000 => "<1k".to_owned(),
        1_000..1_000_000 => format!("{:.0}k", n as f64 / 1_000.0),
        _ => format!("{:.1}M", n as f64 / 1_000_000.0),
    }
}

pub const fn window_is_publishable(people: i64) -> bool {
    people >= MIN_PEOPLE
}
