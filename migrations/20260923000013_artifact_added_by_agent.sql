-- Which agent attached an artifact, when one did. `added_by` stays the
-- agent's owner, who answers for it and is the one who may remove it; this
-- is so the page can say "Attached by Hermes for Anmol".
ALTER TABLE artifact ADD COLUMN added_by_agent_id uuid REFERENCES agent(id);
