-- A person belongs to one department, and a task's discipline is their
-- department. Two facts collapse into one.
--
-- `task.discipline` was set per task and `person.disciplines` was a set per
-- person, so the two could disagree: a task tagged `design` assigned to a
-- backend engineer was a legal state that meant nothing. The department is
-- the fact worth storing; the task's discipline is read off whoever holds it,
-- which makes disagreement unrepresentable.
ALTER TABLE person
  ADD COLUMN department text NOT NULL DEFAULT 'backend'
    CHECK (department IN ('design', 'frontend', 'backend'));

-- Carry across whatever the old set said, first entry wins. The default above
-- covers anyone whose set was empty.
UPDATE person SET department = disciplines[1] WHERE array_length(disciplines, 1) >= 1;

ALTER TABLE person DROP COLUMN disciplines;

-- A manager sees the same board as everyone else; the role is here so the
-- flows that need it later have something to read.
ALTER TABLE person DROP CONSTRAINT person_role_check;
ALTER TABLE person ADD CONSTRAINT person_role_check
  CHECK (role IN ('member', 'manager', 'admin'));

DROP INDEX IF EXISTS task_available_idx;
ALTER TABLE task DROP COLUMN discipline;

CREATE INDEX task_assignee_idx ON task (assignee_person_id)
  WHERE assignee_person_id IS NOT NULL;
