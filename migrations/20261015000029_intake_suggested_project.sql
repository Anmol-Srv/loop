-- The project the filing agent thinks a task belongs in. Accepting from
-- Triage files it there; the owner can still pick another.
ALTER TABLE task_source ADD COLUMN suggested_project_id uuid REFERENCES project(id) ON DELETE SET NULL;
