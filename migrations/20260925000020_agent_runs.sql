-- An agent's runs: one row per pass an agent reports making (an intake agent
-- reading Slack, say), so its owner can see it is alive and what each pass
-- did. Kept to the last 500 per agent by the route that writes them.
CREATE TABLE agent_run (
  id          bigserial PRIMARY KEY,
  agent_id    uuid NOT NULL REFERENCES agent(id) ON DELETE CASCADE,
  started_at  timestamptz NOT NULL DEFAULT now(),
  finished_at timestamptz NOT NULL DEFAULT now(),
  status      text NOT NULL CHECK (status IN ('ok', 'partial', 'failed')),
  summary     text NOT NULL DEFAULT '',
  -- {filed, appended, alreadyFiled, skipped}
  counts      jsonb NOT NULL DEFAULT '{}',
  error       text
);
CREATE INDEX agent_run_agent_idx ON agent_run (agent_id, id DESC);
