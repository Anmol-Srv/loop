-- A handle names a live agent. Revoking one frees its handle, so reconnecting
-- the same tool after a reset is not refused as "already have an agent called
-- 'hermes'" by a row that can never be used again.
ALTER TABLE agent DROP CONSTRAINT agent_owner_id_handle_key;
CREATE UNIQUE INDEX agent_live_handle_idx ON agent (owner_id, handle) WHERE revoked_at IS NULL;
