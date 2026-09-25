-- The earlier messages in the thread a task was filed from, oldest first:
-- [{author, text, ts, receivedAt}], text in Slack formatting with names
-- resolved like `text`. Captured at filing time by the intake agent (it has
-- Slack; the agent that later works the task does not). Private exactly
-- when the message is.
ALTER TABLE task_source ADD COLUMN thread jsonb NOT NULL DEFAULT '[]';
