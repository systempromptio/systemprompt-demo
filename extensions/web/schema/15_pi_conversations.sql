-- Durable pi conversations.
--
-- The homepage terminal used to hold a conversation entirely in the browser and
-- in one process-lifetime `HashMap`, so a reload lost the transcript and, worse,
-- lost the only key that reached its governance spine: `ai_requests` and
-- `governance_decisions` rows survive a refresh, but nothing on disk said which
-- attested session a given conversation belonged to. These two tables are that
-- key plus the transcript itself.
--
-- Deliberately not `session_transcripts` (07_analytics.sql). That table stores
-- one JSONB blob per session, which means every streamed frame would rewrite the
-- whole document, and `Last-Event-ID` resume needs frames addressable by `seq`.

CREATE TABLE IF NOT EXISTS pi_conversations (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- The server-issued session both spines key on. Rewritten on every resume:
    -- a restored conversation gets a fresh attested session, so spend after the
    -- restore joins on the current value while the older rows stay under the
    -- session that produced them.
    attested_session_id TEXT NOT NULL,
    -- NULL until the first user message auto-titles it.
    title TEXT,
    -- Highest `seq` durably written, so a viewer can attach the live stream
    -- exactly where the stored history stops.
    last_seq BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- The child process ended. The row lives on; this is what makes a reload
    -- non-destructive.
    closed_at TIMESTAMPTZ,
    -- Out of the picker and out of focus, still in every tally. Conversations
    -- are archived rather than deleted because the governance rows they explain
    -- outlive them, and the account's telemetry is read across all of them.
    archived_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_pi_conversations_user
    ON pi_conversations(user_id, updated_at DESC)
    WHERE archived_at IS NULL;

-- Every attested session this conversation has ever been bound to.
-- `attested_session_id` above is only the *current* binding; resumes rewrite
-- it, and the spend/governance rows written under earlier sessions would be
-- unreachable without this history. Stat queries key on the conversation and
-- join through here.
CREATE TABLE IF NOT EXISTS pi_conversation_sessions (
    conversation_id TEXT NOT NULL REFERENCES pi_conversations(id) ON DELETE CASCADE,
    session_id TEXT NOT NULL,
    bound_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (conversation_id, session_id)
);

CREATE INDEX IF NOT EXISTS idx_pi_conversation_sessions_session
    ON pi_conversation_sessions(session_id);

CREATE TABLE IF NOT EXISTS pi_conversation_events (
    conversation_id TEXT NOT NULL REFERENCES pi_conversations(id) ON DELETE CASCADE,
    -- The same monotonic per-session counter the SSE stream uses as its event id.
    seq BIGINT NOT NULL,
    at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- The `PiEventBody` tag, denormalised out of `body` so a transcript render
    -- can filter without unpacking every document.
    kind TEXT NOT NULL,
    body JSONB NOT NULL,
    PRIMARY KEY (conversation_id, seq)
);
