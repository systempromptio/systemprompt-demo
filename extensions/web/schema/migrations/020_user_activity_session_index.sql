-- `governance_stats` reads tool fires out of `user_activity` scoped to a single
-- session, and the session id lives in `metadata` rather than in a column:
-- `user_activity` is core-owned, so an index is a safer thing for this repo to
-- add to it than a column.
--
-- No backfill. Rows written before `record_mcp_access` began stamping the
-- session have no `session_id`, and they should fall outside every session's
-- scope rather than be attributed to a guess.
CREATE INDEX IF NOT EXISTS idx_user_activity_mcp_session
    ON user_activity (user_id, (metadata->>'session_id'))
    WHERE category = 'mcp_access';
