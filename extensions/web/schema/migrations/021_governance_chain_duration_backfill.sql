-- Forward-only, idempotent. Safe to re-run.
-- Governance chain duration backfill (2026-07-28): rows written before the
-- policy chain carried timings have chain entries with no duration_ms key.
-- Stamp those entries with duration_ms = 0 so every reader can require the
-- key; 0 is the sentinel for "recorded before timings existed" and renders
-- as an em dash, never as a measured figure.

UPDATE governance_decisions
SET evaluated_rules = jsonb_set(
    evaluated_rules,
    '{chain}',
    (
        SELECT jsonb_agg(
            CASE
                WHEN entry ? 'duration_ms' THEN entry
                ELSE entry || '{"duration_ms": 0}'::jsonb
            END
        )
        FROM jsonb_array_elements(evaluated_rules->'chain') entry
    )
)
WHERE jsonb_typeof(evaluated_rules->'chain') = 'array'
  AND jsonb_array_length(evaluated_rules->'chain') > 0
  AND EXISTS (
      SELECT 1
      FROM jsonb_array_elements(evaluated_rules->'chain') entry
      WHERE NOT entry ? 'duration_ms'
  );
