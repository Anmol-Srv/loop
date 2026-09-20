-- Disciplines: what a person can pick up, and what a task needs.
--
-- A task's discipline is nullable on purpose — not all work is design,
-- frontend or backend, and forcing a label on a chore would make the
-- "available to me" list lie. Existing rows stay null; nobody backfills a
-- guess.

ALTER TABLE person
  ADD COLUMN disciplines text[] NOT NULL DEFAULT '{}'
    CHECK (disciplines <@ ARRAY['design', 'frontend', 'backend']);

ALTER TABLE task
  ADD COLUMN discipline text
    CHECK (discipline IS NULL OR discipline IN ('design', 'frontend', 'backend'));

-- The "available to me" query is: a discipline I hold, nobody assigned, every
-- blocker done. Only the first two are indexable — the blocker check is a
-- NOT EXISTS over `blocked_by`, which is empty for almost every row. Unassigned
-- tasks are the small tail of the table, so a partial index on exactly that
-- predicate keeps the scan proportional to the answer rather than the board.
CREATE INDEX task_available_idx ON task(discipline) WHERE assignee_kind IS NULL;
