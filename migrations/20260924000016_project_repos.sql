-- Repositories: where a project's code lives, so an agent handed one of its
-- tasks knows which folder to work in. The repo is the team's; the folder it
-- is checked out to is each person's own, and only theirs to read.
CREATE TABLE project_repo (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  project_id  uuid NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  name        text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 80),
  url         text NOT NULL CHECK (length(url) BETWEEN 1 AND 500),
  created_by  uuid REFERENCES person(id),
  created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX project_repo_project_idx ON project_repo (project_id, created_at);

-- One person's local checkout of one repo. Private: read only by that person
-- and by an agent acting for them.
CREATE TABLE repo_path (
  repo_id     uuid NOT NULL REFERENCES project_repo(id) ON DELETE CASCADE,
  person_id   uuid NOT NULL REFERENCES person(id) ON DELETE CASCADE,
  path        text NOT NULL CHECK (length(path) BETWEEN 1 AND 500),
  updated_at  timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (repo_id, person_id)
);
