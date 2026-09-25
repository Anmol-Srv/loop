-- Intake: a person's agent files tasks for them from what it reads (Slack
-- first). Intake lands in `triage`; the owner accepts it (-> open) or
-- dismisses it (-> dropped). Nothing else enters or leaves triage.

-- The capability, and the project its filings go to. Created on the first
-- intake; a deleted project clears the link and the next intake makes a
-- fresh one (an archived one is skipped in code).
ALTER TABLE agent
  ADD COLUMN can_intake boolean NOT NULL DEFAULT false,
  ADD COLUMN intake_project_id uuid REFERENCES project(id) ON DELETE SET NULL;

ALTER TABLE task DROP CONSTRAINT task_status_check;
ALTER TABLE task ADD CONSTRAINT task_status_check CHECK (
  status IN ('triage', 'open', 'in_progress', 'blocked', 'dropped', 'completed', 'shipped', 'handoff')
);

ALTER TABLE task ADD COLUMN category text
  CHECK (category IN ('bug', 'feature', 'feedback', 'question', 'chore'));

-- Where a filed task came from. One original per task (`appended` false),
-- plus a row for every later message an agent appended to it. The UNIQUE on
-- (agent_id, source_key) covers both, so the same message can never be filed
-- or appended twice by one agent, however the requests race.
CREATE TABLE task_source (
  task_id      uuid NOT NULL REFERENCES task(id) ON DELETE CASCADE,
  agent_id     uuid NOT NULL REFERENCES agent(id),
  appended     boolean NOT NULL DEFAULT false,
  kind         text NOT NULL CHECK (length(btrim(kind)) BETWEEN 1 AND 40),
  source_key   text NOT NULL CHECK (length(btrim(source_key)) BETWEEN 1 AND 500),
  url          text,
  channel      text,
  channel_name text,
  author       text,
  text         text,
  -- A direct message: its text and author are the owner's (and admins') to read.
  private      boolean NOT NULL DEFAULT false,
  received_at  timestamptz,
  reason       text,
  confidence   real CHECK (confidence BETWEEN 0 AND 1),
  created_at   timestamptz NOT NULL DEFAULT now(),
  UNIQUE (agent_id, source_key)
);
CREATE UNIQUE INDEX task_source_original_idx ON task_source (task_id) WHERE NOT appended;
CREATE INDEX task_source_agent_idx ON task_source (agent_id, created_at) WHERE NOT appended;
