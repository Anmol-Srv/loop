-- Roles as capabilities: an agent works on tasks handed to it (`can_work`),
-- files tasks for its owner (`can_intake`), or both. An intake agent (the
-- Slack Agent) only files; it never takes a hand-off.
ALTER TABLE agent ADD COLUMN can_work boolean NOT NULL DEFAULT true;
UPDATE agent SET can_work = false WHERE can_intake;
