-- Record every attested session a conversation has ever been bound to.
--
-- `pi_conversations.attested_session_id` is rewritten on every resume, so the
-- spend and governance rows written before a reload keyed on a session id no
-- query could reach — the dashboard read zero after F5 even though every row
-- was still in Postgres. This table is the binding history; stat queries key
-- on the conversation and join through it.

CREATE TABLE IF NOT EXISTS pi_conversation_sessions (
    conversation_id TEXT NOT NULL REFERENCES pi_conversations(id) ON DELETE CASCADE,
    session_id TEXT NOT NULL,
    bound_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (conversation_id, session_id)
);

CREATE INDEX IF NOT EXISTS idx_pi_conversation_sessions_session
    ON pi_conversation_sessions(session_id);

-- Seed from the live pointer. Session ids overwritten before this table
-- existed are unrecoverable — nothing else links them to a conversation.
INSERT INTO pi_conversation_sessions (conversation_id, session_id, bound_at)
SELECT id, attested_session_id, created_at FROM pi_conversations
ON CONFLICT DO NOTHING;
