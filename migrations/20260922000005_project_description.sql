-- A project gets a description and a named lead.
--
-- `lead_id` already existed and was never written to; the create form is the
-- first thing that sets it, so it keeps its meaning: one person answerable for
-- the project, not a list of members. Membership is what task assignment is
-- for, and inventing a second, parallel notion of "who is on this" would give
-- the board two answers to the same question.
ALTER TABLE project ADD COLUMN description text NOT NULL DEFAULT '';

-- The list screen sorts by this and it is about to be the main read path.
CREATE INDEX IF NOT EXISTS project_created_idx ON project (created_at DESC);
