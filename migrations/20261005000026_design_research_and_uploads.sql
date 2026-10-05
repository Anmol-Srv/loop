-- Design gets its own longer track: open → in progress → research →
-- completed → handoff → shipped. Shipped now ends both tracks.
ALTER TABLE task DROP CONSTRAINT task_status_check;
ALTER TABLE task ADD CONSTRAINT task_status_check CHECK (
  status IN ('triage', 'open', 'in_progress', 'research', 'blocked', 'dropped', 'completed', 'handoff', 'shipped')
);

-- Design work that finished under the old track ended at `completed`; it is
-- `shipped` now, so it stays finished (its `done_at` is kept).
UPDATE task t SET status = 'shipped'
  FROM person p
 WHERE p.id = t.assignee_person_id AND p.department = 'design'
   AND t.status = 'completed' AND t.done_at IS NOT NULL;

-- Files a person attaches to a task by hand: screenshots for the agent, and
-- markdown docs. `added_by` is who uploaded it (null for intake's files).
ALTER TABLE task_file DROP CONSTRAINT task_file_mime_check;
ALTER TABLE task_file ADD CONSTRAINT task_file_mime_check CHECK (
  mime IN ('image/png', 'image/jpeg', 'image/gif', 'image/webp', 'application/pdf', 'text/markdown', 'text/plain')
);
ALTER TABLE task_file ADD COLUMN added_by uuid REFERENCES person(id) ON DELETE SET NULL;
