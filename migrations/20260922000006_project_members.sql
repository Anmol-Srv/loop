-- A project has members, not a lead.
--
-- The first create form asked for one lead and the owner asked for several
-- people the moment they saw it. Membership is a set, so it gets a join table
-- rather than an array column: the foreign keys mean a revoked person cannot
-- linger as a dangling id, and `ON DELETE CASCADE` on the project side means
-- deleting a project takes its roster with it.
CREATE TABLE project_member (
  project_id  uuid NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  person_id   uuid NOT NULL REFERENCES person(id) ON DELETE CASCADE,
  added_at    timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (project_id, person_id)
);
CREATE INDEX project_member_person_idx ON project_member (person_id);

-- Nothing ever wrote to lead_id except the form this replaces.
ALTER TABLE project DROP COLUMN lead_id;
