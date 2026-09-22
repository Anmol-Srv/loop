-- Two tracks, evidence, notes, and the Linear-shaped project properties.
--
-- A task's track is its assignee's department. Engineering ends when the work
-- is in production; design ends when it has been handed over. The two share
-- the states that mean the same thing and diverge where they do not, which is
-- why `completed` is mid-flow for engineering and terminal for design.
--
--   eng      open -> in_progress -> completed -> shipped
--   design   open -> in_progress -> handoff   -> completed
--   either   blocked, dropped
ALTER TABLE task DROP CONSTRAINT task_status_check;
-- `in_review` was a state nobody used and neither track has; `done` splits
-- into the two terminals, and `completed` is the honest reading of both.
UPDATE task SET status = 'in_progress' WHERE status = 'in_review';
UPDATE task SET status = 'completed' WHERE status = 'done';
ALTER TABLE task ADD CONSTRAINT task_status_check CHECK (
  status IN ('open', 'in_progress', 'blocked', 'dropped', 'completed', 'shipped', 'handoff')
);

-- Evidence: how a task was actually finished. `pr` and `commit` close eng
-- work, `figma` closes design work, `doc` and `link` are context.
ALTER TABLE artifact DROP CONSTRAINT artifact_kind_check;
ALTER TABLE artifact ADD CONSTRAINT artifact_kind_check
  CHECK (kind IN ('pr', 'commit', 'figma', 'doc', 'link'));

-- Why a task was completed with no PR behind it. Nullable, and only ever set
-- on the transition that had no evidence to point at.
ALTER TABLE task ADD COLUMN manual_reason text;

-- The team talking on a task. Not evidence: a note gates nothing.
CREATE TABLE note (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  task_id     uuid NOT NULL REFERENCES task(id) ON DELETE CASCADE,
  author_id   uuid REFERENCES person(id),
  body        text NOT NULL,
  created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX note_task_idx ON note (task_id, created_at);

-- Labels are a shared vocabulary rather than free text on each project, so
-- "Backend" and "backend" cannot become two different labels and a colour
-- means the same thing everywhere.
CREATE TABLE label (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  name        text NOT NULL UNIQUE,
  colour      text NOT NULL DEFAULT 'slate',
  created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE project_label (
  project_id  uuid NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  label_id    uuid NOT NULL REFERENCES label(id) ON DELETE CASCADE,
  PRIMARY KEY (project_id, label_id)
);
CREATE INDEX project_label_label_idx ON project_label (label_id);

-- The Linear properties: when it starts, when it is due, how loud it is.
ALTER TABLE project
  ADD COLUMN start_date  date,
  ADD COLUMN target_date date,
  ADD COLUMN priority    integer NOT NULL DEFAULT 2 CHECK (priority BETWEEN 0 AND 4);
