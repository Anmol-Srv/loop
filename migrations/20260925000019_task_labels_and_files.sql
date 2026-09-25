-- Labels on tasks, from the same shared vocabulary projects use; and the
-- files that came with a message an intake agent filed. Intake stops using a
-- hidden per-owner project: a filed task is standalone and wears its
-- source's label ("Slack") instead.

CREATE TABLE task_label (
  task_id   uuid NOT NULL REFERENCES task(id) ON DELETE CASCADE,
  label_id  uuid NOT NULL REFERENCES label(id) ON DELETE CASCADE,
  PRIMARY KEY (task_id, label_id)
);
CREATE INDEX task_label_label_idx ON task_label (label_id);

-- The bytes live here, not on disk: screenshots from a handful of messages a
-- day, 8 MB at most each. `source_key` is the message (original or appended)
-- the file came with; its `private` decides who may read the file.
-- ponytail: bytea in Postgres; move to object storage if files ever reach GBs.
CREATE TABLE task_file (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  task_id     uuid NOT NULL REFERENCES task(id) ON DELETE CASCADE,
  source_key  text NOT NULL,
  name        text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 255),
  mime        text NOT NULL
                CHECK (mime IN ('image/png', 'image/jpeg', 'image/gif', 'image/webp', 'application/pdf')),
  size        int NOT NULL CHECK (size BETWEEN 1 AND 8388608),
  bytes       bytea NOT NULL,
  width       int,
  height      int,
  created_at  timestamptz NOT NULL DEFAULT now(),
  UNIQUE (task_id, source_key, name)
);

-- ---- data: filed tasks leave their intake projects.

-- An intake project: one an agent links, or one the audit trail says an
-- intake made (an archived one the link has moved past).
CREATE TEMP TABLE intake_project ON COMMIT DROP AS
  SELECT intake_project_id AS id FROM agent WHERE intake_project_id IS NOT NULL
  UNION
  SELECT target_id FROM change WHERE target_type = 'project' AND op = 'create' AND patch ? 'intake_agent';

CREATE TEMP TABLE filed ON COMMIT DROP AS
  SELECT t.id, CASE s.kind WHEN 'github' THEN 'GitHub' ELSE initcap(s.kind) END AS label
    FROM task t
    JOIN task_source s ON s.task_id = t.id AND NOT s.appended
    JOIN phase ph ON ph.id = t.phase_id
   WHERE ph.project_id IN (SELECT id FROM intake_project);

INSERT INTO label (name, colour) SELECT DISTINCT label, 'purple' FROM filed
  ON CONFLICT (name) DO NOTHING;
INSERT INTO task_label (task_id, label_id)
  SELECT f.id, l.id FROM filed f JOIN label l ON l.name = f.label
  ON CONFLICT DO NOTHING;
UPDATE task SET phase_id = NULL WHERE id IN (SELECT id FROM filed);

-- Archived, not deleted, and only once nothing live is left in it: a task a
-- person added to one by hand stays where they put it.
UPDATE project pr SET archived_at = now()
 WHERE pr.id IN (SELECT id FROM intake_project)
   AND pr.archived_at IS NULL
   AND NOT EXISTS (SELECT 1 FROM phase ph JOIN task t ON t.phase_id = ph.id
                    WHERE ph.project_id = pr.id AND t.archived_at IS NULL);
-- ponytail: the column stays for one release so a rollback has somewhere to
-- point; nothing reads or writes it now.
UPDATE agent SET intake_project_id = NULL WHERE intake_project_id IS NOT NULL;
