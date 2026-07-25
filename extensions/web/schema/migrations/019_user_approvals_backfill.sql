-- Manual account review gate: every account now needs an approval decision
-- before it can reach the admin plane or draw its signup credit.
--
-- Every account that predates the gate is grandfathered in as approved. Without
-- this backfill the middleware would pin the entire existing user base — the
-- bootstrapped admin included — to /admin/pending with nobody left able to
-- approve them.
--
-- The CREATE is repeated from 13_web_side_tables.sql because migrations run
-- ahead of the declarative pass on an existing database, and the INSERT below
-- needs the table to exist now.

CREATE TABLE IF NOT EXISTS user_approvals (
    user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'pending',
    requested_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    decided_at TIMESTAMPTZ,
    decided_by TEXT,
    denial_reason TEXT
);

CREATE INDEX IF NOT EXISTS idx_user_approvals_status ON user_approvals(status);

INSERT INTO user_approvals (user_id, status, requested_at, decided_at, decided_by)
SELECT id, 'approved', COALESCE(created_at, CURRENT_TIMESTAMP), CURRENT_TIMESTAMP, 'migration'
FROM users
ON CONFLICT (user_id) DO NOTHING;
