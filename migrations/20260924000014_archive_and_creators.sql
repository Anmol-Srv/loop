-- Archiving is a timestamp beside status, not a status. Restoring clears it
-- and the status was never touched, so there is no prior value to remember;
-- one nullable column on both tables means one filter shape for both. The
-- old 'archived' status becomes that timestamp and leaves the vocabulary.
ALTER TABLE project
  ADD COLUMN archived_at timestamptz,
  ADD COLUMN created_by  uuid REFERENCES person(id);
ALTER TABLE task
  ADD COLUMN archived_at timestamptz,
  ADD COLUMN created_by  uuid REFERENCES person(id);

UPDATE project SET archived_at = updated_at, status = 'done' WHERE status = 'archived';
ALTER TABLE project DROP CONSTRAINT project_status_check;
ALTER TABLE project ADD CONSTRAINT project_status_check
  CHECK (status IN ('active', 'paused', 'done'));

-- Who made each row, from its first create in the audit trail. A human's
-- `actor` is their email; anything else (a token, a replayed proposal) falls
-- back to `on_behalf_of`, which on a proposal is the proposer. No row found
-- stays NULL, which leaves the row to an admin.
UPDATE project p SET created_by = c.person
  FROM (SELECT DISTINCT ON (c.target_id) c.target_id, coalesce(per.id, c.on_behalf_of) AS person
          FROM change c LEFT JOIN person per ON per.email = c.actor
         WHERE c.target_type = 'project' AND c.op = 'create' AND c.state IN ('applied', 'approved')
         ORDER BY c.target_id, c.created_at) c
 WHERE c.target_id = p.id;
UPDATE task t SET created_by = c.person
  FROM (SELECT DISTINCT ON (c.target_id) c.target_id, coalesce(per.id, c.on_behalf_of) AS person
          FROM change c LEFT JOIN person per ON per.email = c.actor
         WHERE c.target_type = 'task' AND c.op = 'create' AND c.state IN ('applied', 'approved')
         ORDER BY c.target_id, c.created_at) c
 WHERE c.target_id = t.id;
