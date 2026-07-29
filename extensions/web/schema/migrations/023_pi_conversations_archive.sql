-- Conversations are archived, never deleted.
--
-- `deleted_at` implied a conversation could leave the record, but the rows it
-- explains -- `ai_requests`, `governance_decisions`, `user_activity` -- never
-- do, and the account's telemetry is read across all of them. Archiving is the
-- honest name for what the column always did: drop the conversation out of the
-- picker and out of focus, keep every number it produced.

ALTER TABLE pi_conversations
    ADD COLUMN IF NOT EXISTS archived_at TIMESTAMPTZ;

-- The backfill has to name a column this migration then drops, so it can only
-- run while that column is still there. Guarded rather than assumed: a
-- database that already made the move re-runs this file as a no-op.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'pi_conversations' AND column_name = 'deleted_at'
    ) THEN
        UPDATE pi_conversations
        SET archived_at = deleted_at
        WHERE deleted_at IS NOT NULL AND archived_at IS NULL;
    END IF;
END $$;

DROP INDEX IF EXISTS idx_pi_conversations_user;

ALTER TABLE pi_conversations
    DROP COLUMN IF EXISTS deleted_at;

CREATE INDEX IF NOT EXISTS idx_pi_conversations_user
    ON pi_conversations(user_id, updated_at DESC)
    WHERE archived_at IS NULL;
