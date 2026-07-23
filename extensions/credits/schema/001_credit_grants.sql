-- Credit ledger: one row per grant of credit to a user. Balance is the sum of
-- grants minus the user's recorded AI-request cost (see get_balance_microdollars).
CREATE TABLE IF NOT EXISTS credit_grants (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id VARCHAR(255) NOT NULL,
    microdollars BIGINT NOT NULL,
    reason TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- One grant per (user, reason) makes signup grants idempotent.
    UNIQUE (user_id, reason)
);

CREATE INDEX IF NOT EXISTS idx_credit_grants_user ON credit_grants(user_id);
