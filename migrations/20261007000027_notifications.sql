-- Notifications: what happened on your work while you were elsewhere.
--
-- Written by triggers on the three rows every change already leaves behind —
-- the audit `change` row (who did it, and what), a `note`, a `task_plan` — so
-- no write path can forget to notify. Never about your own action.
CREATE TABLE notification (
  id         bigserial PRIMARY KEY,
  person_id  uuid NOT NULL REFERENCES person(id) ON DELETE CASCADE,
  -- assigned | status | note | question | plan | submitted | unblocked
  kind       text NOT NULL,
  task_id    uuid NOT NULL REFERENCES task(id) ON DELETE CASCADE,
  actor      text NOT NULL,
  -- The detail line: the new status, the note's first words.
  detail     text NOT NULL DEFAULT '',
  created_at timestamptz NOT NULL DEFAULT now(),
  read_at    timestamptz
);
CREATE INDEX notification_person_idx ON notification (person_id, id DESC);
CREATE INDEX notification_unread_idx ON notification (person_id) WHERE read_at IS NULL;

CREATE FUNCTION notify(who uuid, k text, t uuid, actor text, detail text) RETURNS void AS $$
BEGIN
  IF who IS NOT NULL THEN
    INSERT INTO notification (person_id, kind, task_id, actor, detail)
    VALUES (who, k, t, actor, left(coalesce(detail, ''), 200));
  END IF;
END $$ LANGUAGE plpgsql;

-- The name of whoever made this change: the agent, inside an agent's
-- transaction; else the person.
CREATE FUNCTION change_actor_name(on_behalf uuid) RETURNS text AS $$
  SELECT coalesce(
    (SELECT name FROM agent WHERE id = nullif(current_setting('acp.agent_id', true), '')::uuid),
    (SELECT name FROM person WHERE id = on_behalf),
    'Someone')
$$ LANGUAGE sql STABLE;

CREATE FUNCTION notify_on_change() RETURNS trigger AS $$
DECLARE
  t record;
  actor text;
  by_agent boolean := nullif(current_setting('acp.agent_id', true), '') IS NOT NULL;
  b record;
BEGIN
  IF NEW.target_type <> 'task' OR NEW.state <> 'applied' THEN
    RETURN NEW;
  END IF;
  SELECT id, title, status, assignee_person_id INTO t FROM task WHERE id = NEW.target_id;
  IF NOT FOUND THEN
    RETURN NEW;
  END IF;
  actor := change_actor_name(NEW.on_behalf_of);

  -- Given to you: made with you as assignee, or (re)assigned to you.
  IF (NEW.op = 'create'
      OR NEW.patch ? 'person_email'
      OR coalesce(NEW.patch -> 'details' ->> 'assigneeId', '') <> '')
     AND t.assignee_person_id IS DISTINCT FROM NEW.on_behalf_of THEN
    PERFORM notify(t.assignee_person_id, 'assigned', t.id, actor, '');
  END IF;

  IF NEW.op = 'update' AND NEW.patch ? 'status' THEN
    -- Someone else moved your task. An agent's own moves show in its notes.
    IF NOT by_agent AND t.assignee_person_id IS DISTINCT FROM NEW.on_behalf_of THEN
      PERFORM notify(t.assignee_person_id, 'status', t.id, actor, t.status);
    END IF;
    -- It no longer holds anyone up: tell whoever was waiting on it.
    IF t.status IN ('handoff', 'completed', 'shipped', 'dropped') THEN
      FOR b IN SELECT id, assignee_person_id FROM task
                WHERE t.id = ANY(blocked_by) AND archived_at IS NULL
                  AND status NOT IN ('completed', 'shipped', 'dropped') LOOP
        IF b.assignee_person_id IS DISTINCT FROM NEW.on_behalf_of THEN
          PERFORM notify(b.assignee_person_id, 'unblocked', b.id, actor, t.title);
        END IF;
      END LOOP;
    END IF;
  END IF;
  RETURN NEW;
END $$ LANGUAGE plpgsql;

CREATE TRIGGER change_notify AFTER INSERT ON change
  FOR EACH ROW EXECUTE FUNCTION notify_on_change();

CREATE FUNCTION notify_on_note() RETURNS trigger AS $$
DECLARE
  owner uuid;
  actor text;
BEGIN
  SELECT assignee_person_id INTO owner FROM task WHERE id = NEW.task_id;
  IF NEW.agent_id IS NOT NULL THEN
    actor := coalesce((SELECT name FROM agent WHERE id = NEW.agent_id), 'Your agent');
    IF NEW.kind = 'question' THEN
      PERFORM notify(owner, 'question', NEW.task_id, actor, NEW.body);
    ELSIF NEW.kind = 'submission' THEN
      PERFORM notify(owner, 'submitted', NEW.task_id, actor, NEW.body);
    END IF;
  ELSIF NEW.kind = 'note' AND owner IS DISTINCT FROM NEW.author_id THEN
    actor := coalesce((SELECT name FROM person WHERE id = NEW.author_id), 'Someone');
    PERFORM notify(owner, 'note', NEW.task_id, actor, NEW.body);
  END IF;
  RETURN NEW;
END $$ LANGUAGE plpgsql;

CREATE TRIGGER note_notify AFTER INSERT ON note
  FOR EACH ROW EXECUTE FUNCTION notify_on_note();

CREATE FUNCTION notify_on_plan() RETURNS trigger AS $$
BEGIN
  PERFORM notify((SELECT assignee_person_id FROM task WHERE id = NEW.task_id), 'plan', NEW.task_id,
                 coalesce((SELECT name FROM agent WHERE id = NEW.agent_id), 'Your agent'), NEW.summary);
  RETURN NEW;
END $$ LANGUAGE plpgsql;

CREATE TRIGGER task_plan_notify AFTER INSERT ON task_plan
  FOR EACH ROW EXECUTE FUNCTION notify_on_plan();
