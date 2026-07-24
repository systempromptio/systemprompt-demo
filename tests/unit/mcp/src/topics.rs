//! Tests for the `systemprompt-mcp-agent` documentation hub registry: topic
//! lookup and keyword-scoring search.

use systemprompt_mcp_agent::topics;

#[test]
fn every_topic_has_a_unique_nonempty_id() {
    let mut ids: Vec<&str> = topics::TOPICS.iter().map(|t| t.id).collect();
    assert!(!ids.is_empty(), "registry must not be empty");
    ids.sort_unstable();
    let before = ids.len();
    ids.dedup();
    assert_eq!(before, ids.len(), "topic ids must be unique");
    for topic in topics::TOPICS {
        assert!(!topic.id.is_empty(), "topic id must not be empty");
        assert!(!topic.title.is_empty(), "topic title must not be empty");
        assert!(!topic.body.is_empty(), "topic body must not be empty");
    }
}

#[test]
fn find_returns_the_matching_topic_or_none() {
    assert!(topics::find("governance-pipeline").is_some());
    assert!(topics::find("does-not-exist").is_none());
}

#[test]
fn search_ranks_the_most_relevant_topic_first() {
    let hits = topics::search("how are secrets blocked in the governance pipeline");
    assert!(!hits.is_empty(), "expected at least one hit");
    assert_eq!(
        hits[0].topic.id, "governance-pipeline",
        "the governance topic should rank first for a governance query"
    );
    // scores are non-increasing
    for pair in hits.windows(2) {
        assert!(
            pair[0].score >= pair[1].score,
            "hits must be sorted by score"
        );
    }
}

#[test]
fn search_ignores_noise_and_returns_nothing_for_unrelated_queries() {
    assert!(
        topics::search("a").is_empty(),
        "single-char tokens are ignored"
    );
    assert!(
        topics::search("zzzzqqqq nonsense").is_empty(),
        "unrelated query should match no topics"
    );
}
