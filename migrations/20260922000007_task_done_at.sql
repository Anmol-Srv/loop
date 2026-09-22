-- When a task was finished, set on the transition to done and cleared if it
-- is reopened. The dashboard's "completed this week" was bucketing on
-- updated_at, which moves for any edit; this is the real date.
ALTER TABLE task ADD COLUMN done_at timestamptz;
CREATE INDEX task_done_at_idx ON task (done_at) WHERE done_at IS NOT NULL;
