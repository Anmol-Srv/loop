-- Agent sessions: what an agent is doing right now, when it was handed the
-- task, what its setup reported, and private instructions from its owner.

-- The "now" line: one short sentence while the agent works. Only meaningful
-- while it holds the task and is on it, so the trigger below clears it the
-- moment `agent_state` is anything else, whichever path moved it.
ALTER TABLE task
  ADD COLUMN agent_now     text,
  ADD COLUMN agent_now_at  timestamptz,
  ADD COLUMN delegated_at  timestamptz;

UPDATE task t SET delegated_at = e.created_at
  FROM (SELECT DISTINCT ON (task_id) task_id, created_at FROM agent_event
         WHERE kind = 'handed_off' ORDER BY task_id, id DESC) e
 WHERE e.task_id = t.id;

-- Named to sort after `task_agent_event`: BEFORE triggers fire by name, and
-- that one may stop the agent (a drop, a reassignment) in the same write.
CREATE FUNCTION task_agent_session() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF coalesce(NEW.agent_state, '') NOT IN ('working', 'acknowledged') THEN
    NEW.agent_now := NULL;
    NEW.agent_now_at := NULL;
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER task_agent_session BEFORE UPDATE ON task
  FOR EACH ROW EXECUTE FUNCTION task_agent_session();

-- What the agent reported at hello: {skill, mcp, watcher}.
ALTER TABLE agent ADD COLUMN setup jsonb NOT NULL DEFAULT '{}';

-- An owner's private instruction to their agent.
ALTER TABLE note DROP CONSTRAINT note_kind_check;
ALTER TABLE note ADD CONSTRAINT note_kind_check CHECK (kind IN
  ('note', 'progress', 'question', 'answer', 'submission', 'review', 'instruction'));

CREATE OR REPLACE FUNCTION note_agent_event() RETURNS trigger LANGUAGE plpgsql AS $$
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
      WHEN 'instruction' THEN 'instruction'
      WHEN 'review' THEN CASE WHEN s = 'done' THEN 'approved' ELSE 'changes_requested' END
      ELSE 'note'
    END,
    jsonb_build_object('noteId', NEW.id, 'body', NEW.body,
                       'author', (SELECT name FROM person WHERE id = NEW.author_id)));
  RETURN NULL;
END $$;
