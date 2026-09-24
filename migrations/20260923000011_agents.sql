-- Agents are rows, not tokens. A personal agent (Hermes, Claude Code, Codex)
-- is something a person owns and hands their own tasks to; its credential is
-- just how it proves who it is, and rotates under it.
CREATE TABLE agent (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id     uuid NOT NULL REFERENCES person(id),
  handle       text NOT NULL,
  name         text NOT NULL,
  runtime      text NOT NULL DEFAULT 'other'
                 CHECK (runtime IN ('hermes', 'claude-code', 'codex', 'other')),
  connected_at timestamptz,
  last_seen_at timestamptz,
  -- The last agent_event id it acknowledged.
  event_cursor bigint NOT NULL DEFAULT 0,
  revoked_at   timestamptz,
  created_at   timestamptz NOT NULL DEFAULT now(),
  UNIQUE (owner_id, handle)
);

ALTER TABLE credential ADD COLUMN agent_id uuid REFERENCES agent(id);
CREATE INDEX credential_agent_idx ON credential (agent_id) WHERE agent_id IS NOT NULL;

-- One agent per existing agent credential, named after its label. Two labels
-- the same under one owner get a numeric suffix rather than failing the
-- UNIQUE above. The CTE is materialised once (it calls a volatile function),
-- so the ids the agents get are the ids the credentials point at.
WITH src AS (
  SELECT c.id AS credential_id, gen_random_uuid() AS agent_id, c.owner_id, c.label,
         c.revoked_at, c.last_used_at, c.created_at,
         row_number() OVER (PARTITION BY c.owner_id, c.label ORDER BY c.created_at) AS n
    FROM credential c
   WHERE c.kind = 'agent'
), made AS (
  INSERT INTO agent (id, owner_id, handle, name, connected_at, last_seen_at, revoked_at, created_at)
  SELECT agent_id, owner_id, label || CASE WHEN n > 1 THEN '-' || n ELSE '' END, label,
         last_used_at, last_used_at, revoked_at, created_at
    FROM src
)
UPDATE credential c SET agent_id = src.agent_id FROM src WHERE c.id = src.credential_id;

ALTER TABLE credential ADD CONSTRAINT agent_credential_has_agent
  CHECK (kind <> 'agent' OR agent_id IS NOT NULL);

-- A handed-off task keeps its human assignee: their department still picks
-- the track and their name still shows as owner. The agent is a delegate.
ALTER TABLE task
  ADD COLUMN delegate_agent_id uuid REFERENCES agent(id),
  ADD COLUMN agent_state text CHECK (agent_state IN
    ('handed_off', 'acknowledged', 'working', 'needs_input', 'in_review', 'done', 'stopped')),
  -- The finishing status the agent asked to move to; applied on approval.
  ADD COLUMN review_target text;
CREATE INDEX task_delegate_idx ON task (delegate_agent_id) WHERE delegate_agent_id IS NOT NULL;

-- Agent entries live beside the team's notes, so they appear where people
-- already read. An agent's note has no person author.
ALTER TABLE note
  ADD COLUMN agent_id uuid REFERENCES agent(id),
  ADD COLUMN kind text NOT NULL DEFAULT 'note'
    CHECK (kind IN ('note', 'progress', 'question', 'answer', 'submission', 'review')),
  ADD CONSTRAINT note_one_author CHECK (author_id IS NULL OR agent_id IS NULL);

-- What reached an agent from the dashboard, in order. Its feed is `id > cursor`.
CREATE TABLE agent_event (
  id         bigserial PRIMARY KEY,
  agent_id   uuid NOT NULL REFERENCES agent(id) ON DELETE CASCADE,
  task_id    uuid NOT NULL REFERENCES task(id) ON DELETE CASCADE,
  kind       text NOT NULL,
  payload    jsonb NOT NULL DEFAULT '{}',
  created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX agent_event_feed_idx ON agent_event (agent_id, id);

-- Events are made by triggers, in one place, so no route can forget one.
-- Agent routes set `acp.agent_id` for their transaction, and a change made
-- under the delegate's own id is not echoed back to it.
CREATE FUNCTION agent_event_emit(a uuid, t uuid, k text, p jsonb) RETURNS void
LANGUAGE plpgsql AS $$
BEGIN
  INSERT INTO agent_event (agent_id, task_id, kind, payload) VALUES (a, t, k, p);
  PERFORM pg_notify('agent_event', a::text);
END $$;

CREATE FUNCTION acting_agent() RETURNS uuid LANGUAGE sql STABLE AS $$
  SELECT nullif(current_setting('acp.agent_id', true), '')::uuid
$$;

-- BEFORE, so a drop can also stop the agent in the same row write.
CREATE FUNCTION task_agent_event() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
  changed jsonb := '{}';
BEGIN
  -- A delegate works for the assignee, so a new assignee (or none) takes the
  -- task back from the old one's agent, whichever route reassigned it.
  IF NEW.assignee_person_id IS DISTINCT FROM OLD.assignee_person_id
     AND NEW.delegate_agent_id IS NOT DISTINCT FROM OLD.delegate_agent_id
     AND NEW.delegate_agent_id IS NOT NULL THEN
    NEW.delegate_agent_id := NULL;
    NEW.agent_state := 'stopped';
    NEW.review_target := NULL;
  END IF;

  -- Handing off, taking back, or handing the same agent the task again after
  -- it had finished or been stopped.
  IF NEW.delegate_agent_id IS DISTINCT FROM OLD.delegate_agent_id
     OR (NEW.agent_state = 'handed_off' AND OLD.agent_state IS DISTINCT FROM 'handed_off') THEN
    IF OLD.delegate_agent_id IS NOT NULL
       AND OLD.delegate_agent_id IS DISTINCT FROM NEW.delegate_agent_id THEN
      PERFORM agent_event_emit(OLD.delegate_agent_id, NEW.id, 'taken_back', '{}');
    END IF;
    IF NEW.delegate_agent_id IS NOT NULL THEN
      PERFORM agent_event_emit(NEW.delegate_agent_id, NEW.id, 'handed_off',
                               jsonb_build_object('title', NEW.title));
    END IF;
    RETURN NEW;
  END IF;

  IF NEW.delegate_agent_id IS NULL OR NEW.delegate_agent_id IS NOT DISTINCT FROM acting_agent() THEN
    RETURN NEW;
  END IF;

  IF NEW.status = 'dropped' AND OLD.status <> 'dropped' THEN
    NEW.agent_state := 'stopped';
    PERFORM agent_event_emit(NEW.delegate_agent_id, NEW.id, 'dropped', '{}');
    RETURN NEW;
  END IF;

  IF NEW.title IS DISTINCT FROM OLD.title THEN
    changed := changed || jsonb_build_object('title', NEW.title);
  END IF;
  IF NEW.body IS DISTINCT FROM OLD.body THEN
    changed := changed || jsonb_build_object('body', NEW.body);
  END IF;
  IF NEW.priority IS DISTINCT FROM OLD.priority THEN
    changed := changed || jsonb_build_object('priority', NEW.priority);
  END IF;
  IF NEW.status IS DISTINCT FROM OLD.status THEN
    changed := changed || jsonb_build_object('status', NEW.status);
  END IF;
  IF NEW.blocked_by IS DISTINCT FROM OLD.blocked_by THEN
    changed := changed || jsonb_build_object('blocked_by', to_jsonb(NEW.blocked_by));
  END IF;
  IF changed <> '{}' THEN
    PERFORM agent_event_emit(NEW.delegate_agent_id, NEW.id, 'changed', jsonb_build_object(
      'fields', (SELECT jsonb_agg(k ORDER BY k) FROM jsonb_object_keys(changed) k),
      'values', changed));
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER task_agent_event BEFORE UPDATE ON task
  FOR EACH ROW EXECUTE FUNCTION task_agent_event();

-- An owner's answer or review is a note too; its kind says which event it is.
-- A review is written after the task's state has moved, so `done` means
-- the owner approved and anything else means they asked for changes.
CREATE FUNCTION note_agent_event() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
  d uuid;
  s text;
BEGIN
  SELECT delegate_agent_id, agent_state INTO d, s FROM task WHERE id = NEW.task_id;
  IF d IS NULL OR d IS NOT DISTINCT FROM acting_agent() THEN
    RETURN NULL;
  END IF;
  PERFORM agent_event_emit(d, NEW.task_id,
    CASE NEW.kind
      WHEN 'answer' THEN 'answer'
      WHEN 'review' THEN CASE WHEN s = 'done' THEN 'approved' ELSE 'changes_requested' END
      ELSE 'note'
    END,
    jsonb_build_object('noteId', NEW.id, 'body', NEW.body,
                       'author', (SELECT name FROM person WHERE id = NEW.author_id)));
  RETURN NULL;
END $$;
CREATE TRIGGER note_agent_event AFTER INSERT ON note
  FOR EACH ROW EXECUTE FUNCTION note_agent_event();

CREATE FUNCTION artifact_agent_event() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
  d uuid;
BEGIN
  IF NEW.parent_type <> 'task' THEN
    RETURN NULL;
  END IF;
  SELECT delegate_agent_id INTO d FROM task WHERE id = NEW.parent_id;
  IF d IS NULL OR d IS NOT DISTINCT FROM acting_agent() THEN
    RETURN NULL;
  END IF;
  PERFORM agent_event_emit(d, NEW.parent_id, 'artifact', jsonb_build_object(
    'artifactId', NEW.id, 'kind', NEW.kind, 'url', NEW.url, 'title', NEW.title));
  RETURN NULL;
END $$;
CREATE TRIGGER artifact_agent_event AFTER INSERT ON artifact
  FOR EACH ROW EXECUTE FUNCTION artifact_agent_event();
