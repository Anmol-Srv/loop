-- A person's own folders their agent can work in when a task has no project
-- (or its project has no folder for them): named, so a task can pin one by
-- name regardless of which machine that person's agent runs on. Private,
-- like a repo's local path — only the owner reads or edits their own.
CREATE TABLE user_folder (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  person_id   uuid NOT NULL REFERENCES person(id) ON DELETE CASCADE,
  name        text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 80),
  path        text NOT NULL CHECK (length(path) BETWEEN 1 AND 500),
  is_default  boolean NOT NULL DEFAULT false,
  created_at  timestamptz NOT NULL DEFAULT now(),
  UNIQUE (person_id, name)
);
-- At most one default per person.
CREATE UNIQUE INDEX user_folder_one_default_idx ON user_folder (person_id) WHERE is_default;

-- A task pinned to one of its assignee's folders by name, for when it has no
-- project (or its project's repo has no folder for them). The name, not the
-- id: folders are per person, and whichever agent the task is later
-- delegated to resolves it against their own list.
ALTER TABLE task ADD COLUMN folder_name text;
