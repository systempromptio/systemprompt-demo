-- Per-IP friction on self-registration.
--
-- Signup grants an AI credit immediately, and the pre-existing limiter counts
-- `webauthn_setup_tokens` per email — which anyone sidesteps by varying the
-- email. The address that made the request is the only identifier left worth
-- capping.
--
-- Deliberately not a column on `webauthn_setup_tokens`: that table is
-- core-owned and the demo must not widen it. Deliberately not keyed to a user
-- either, because a refused attempt never creates a user row.

CREATE TABLE IF NOT EXISTS registration_attempts (
    id BIGSERIAL PRIMARY KEY,
    -- TEXT rather than INET: the only writer is a parsed `IpAddr` rendered by
    -- `to_string`, so the value is already canonical and INET's normalisation
    -- would buy nothing that the type system has not already guaranteed. It
    -- would cost an `ipnetwork` feature on the workspace `sqlx` dependency,
    -- since a bound INET parameter has no mapping without one.
    ip_address TEXT NOT NULL,
    email TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_registration_attempts_ip_recent
    ON registration_attempts(ip_address, created_at DESC);
