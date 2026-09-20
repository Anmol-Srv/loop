CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE person (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  email       text NOT NULL UNIQUE,
  name        text NOT NULL,
  role        text NOT NULL DEFAULT 'member' CHECK (role IN ('member', 'admin')),
  deleted_at  timestamptz,
  created_at  timestamptz NOT NULL DEFAULT now(),
  updated_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE agent_token (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  label       text NOT NULL,
  token_hash  text NOT NULL UNIQUE,
  owner_id    uuid NOT NULL REFERENCES person(id),
  scopes      text[] NOT NULL DEFAULT '{}',
  expires_at  timestamptz NOT NULL,
  revoked_at  timestamptz,
  created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX agent_token_owner_idx ON agent_token(owner_id);

CREATE TABLE project (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  key         text NOT NULL UNIQUE,
  name        text NOT NULL,
  status      text NOT NULL DEFAULT 'active'
                CHECK (status IN ('active', 'paused', 'done', 'archived')),
  lead_id     uuid REFERENCES person(id),
  created_at  timestamptz NOT NULL DEFAULT now(),
  updated_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE phase (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  project_id  uuid NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  position    integer NOT NULL,
  name        text NOT NULL,
  status      text NOT NULL DEFAULT 'planned'
                CHECK (status IN ('planned', 'active', 'blocked', 'done')),
  gate        boolean NOT NULL DEFAULT false,
  created_at  timestamptz NOT NULL DEFAULT now(),
  updated_at  timestamptz NOT NULL DEFAULT now(),
  UNIQUE (project_id, position)
);

CREATE TABLE task (
  id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  phase_id          uuid NOT NULL REFERENCES phase(id) ON DELETE CASCADE,
  title             text NOT NULL,
  body              text NOT NULL DEFAULT '',
  status            text NOT NULL DEFAULT 'open'
                      CHECK (status IN ('open', 'in_progress', 'in_review', 'blocked', 'done', 'dropped')),
  priority          integer NOT NULL DEFAULT 2 CHECK (priority BETWEEN 0 AND 4),
  assignee_kind     text CHECK (assignee_kind IN ('human', 'agent')),
  assignee_person_id uuid REFERENCES person(id),
  assignee_token_id  uuid REFERENCES agent_token(id),
  claimed_by        text,
  claim_expires_at  timestamptz,
  blocked_by        uuid[] NOT NULL DEFAULT '{}',
  created_at        timestamptz NOT NULL DEFAULT now(),
  updated_at        timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT assignee_matches_kind CHECK (
    (assignee_kind IS NULL  AND assignee_person_id IS NULL AND assignee_token_id IS NULL) OR
    (assignee_kind = 'human' AND assignee_person_id IS NOT NULL AND assignee_token_id IS NULL) OR
    (assignee_kind = 'agent' AND assignee_token_id IS NOT NULL AND assignee_person_id IS NULL)
  )
);
CREATE INDEX task_phase_idx ON task(phase_id);
CREATE INDEX task_status_idx ON task(status);
CREATE INDEX task_claimable_idx ON task(assignee_kind, status) WHERE assignee_kind = 'agent';

CREATE TABLE artifact (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  parent_type  text NOT NULL CHECK (parent_type IN ('project', 'phase', 'task')),
  parent_id    uuid NOT NULL,
  kind         text NOT NULL CHECK (kind IN ('pr', 'doc', 'link')),
  url          text NOT NULL,
  title        text NOT NULL DEFAULT '',
  metadata     jsonb NOT NULL DEFAULT '{}',
  added_by     uuid REFERENCES person(id),
  created_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX artifact_parent_idx ON artifact(parent_type, parent_id);

CREATE TABLE change (
  id             uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  actor          text NOT NULL,
  on_behalf_of   uuid REFERENCES person(id),
  target_type    text NOT NULL CHECK (target_type IN ('project', 'phase', 'task', 'artifact')),
  target_id      uuid NOT NULL,
  op             text NOT NULL CHECK (op IN ('create', 'update', 'delete')),
  patch          jsonb NOT NULL,
  state          text NOT NULL DEFAULT 'pending'
                   CHECK (state IN ('pending', 'applied', 'approved', 'rejected')),
  applied_at     timestamptz,
  created_at     timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX change_target_idx ON change(target_type, target_id);
CREATE INDEX change_pending_idx ON change(state) WHERE state = 'pending';

CREATE TABLE job (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  kind        text NOT NULL,
  payload     jsonb NOT NULL DEFAULT '{}',
  run_after   timestamptz NOT NULL DEFAULT now(),
  attempts    integer NOT NULL DEFAULT 0,
  locked_by   text,
  locked_at   timestamptz,
  created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX job_ready_idx ON job(run_after) WHERE locked_by IS NULL;

CREATE TABLE run_log_line (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  task_id     uuid NOT NULL REFERENCES task(id) ON DELETE CASCADE,
  seq         bigint NOT NULL,
  text        text NOT NULL,
  created_at  timestamptz NOT NULL DEFAULT now(),
  UNIQUE (task_id, seq)
);
