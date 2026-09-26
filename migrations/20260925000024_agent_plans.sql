-- A plan the agent sends before it builds, and the owner's optional note at
-- hand-off.
--
-- The gate: a hand-off carries no approval, so `plan_approved_at` starts NULL.
-- `task_plan` sends the owner a short summary plus the concrete steps; every
-- call is a new revision, so nothing already sent is overwritten and the
-- newest row for a task is the current plan. Approving stamps
-- `plan_approved_at`; a fresh `task_plan` clears it again, so an approval
-- never outlives the revision it was given for. `task_submit` and attaching a
-- `pr` are refused in Rust (controllers::agent) until it is set — the same
-- place the other finishing checks already live.

ALTER TABLE task DROP CONSTRAINT task_agent_state_check;
ALTER TABLE task ADD CONSTRAINT task_agent_state_check CHECK (agent_state IN
  ('handed_off', 'acknowledged', 'plan_review', 'working', 'needs_input', 'in_review', 'done', 'stopped'));

ALTER TABLE task
  ADD COLUMN plan_approved_at timestamptz,
  -- The owner's optional note at hand-off: context the task itself doesn't
  -- say. Scoped to the current delegation, like the plan gate above — cleared
  -- on take-back and set fresh on every hand-off.
  ADD COLUMN brief text CHECK (brief IS NULL OR length(brief) <= 8000);

-- Revisions: every task_plan call is a new row, so "Earlier plans" is just the
-- rows before the latest. `decision` is NULL while it waits on the owner.
CREATE TABLE task_plan (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  task_id      uuid NOT NULL REFERENCES task(id) ON DELETE CASCADE,
  agent_id     uuid NOT NULL REFERENCES agent(id),
  summary      text NOT NULL CHECK (length(summary) BETWEEN 1 AND 600),
  plan         text NOT NULL CHECK (length(plan) BETWEEN 1 AND 20000),
  decision     text CHECK (decision IN ('approved', 'changes_requested')),
  decided_at   timestamptz,
  changes_note text,
  created_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX task_plan_task_idx ON task_plan (task_id, created_at);

-- A delegation ends its plan and its brief along with it: reassigning without
-- an explicit take-back stops the old delegate the same way `take_back` and
-- `revoke` do (controllers::agent), so this trigger clears the same two
-- columns. Otherwise identical to the function `20260923000011_agents.sql`
-- defined.
CREATE OR REPLACE FUNCTION task_agent_event() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
  changed jsonb := '{}';
BEGIN
  IF NEW.assignee_person_id IS DISTINCT FROM OLD.assignee_person_id
     AND NEW.delegate_agent_id IS NOT DISTINCT FROM OLD.delegate_agent_id
     AND NEW.delegate_agent_id IS NOT NULL THEN
    NEW.delegate_agent_id := NULL;
    NEW.agent_state := 'stopped';
    NEW.review_target := NULL;
    NEW.brief := NULL;
    NEW.plan_approved_at := NULL;
  END IF;

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
