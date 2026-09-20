-- Passwords for people, and one credential table that says what each row is.

ALTER TABLE person
  ADD COLUMN password_hash   text,
  ADD COLUMN password_set_at timestamptz,
  ADD COLUMN failed_attempts integer NOT NULL DEFAULT 0,
  ADD COLUMN locked_until    timestamptz;

-- `agent_token` was doing two jobs: a person's session and an agent's
-- identity. One table still, but it now says which it is.
ALTER TABLE agent_token RENAME TO credential;
ALTER TABLE credential
  ADD COLUMN kind         text NOT NULL DEFAULT 'agent'
    CHECK (kind IN ('session', 'agent')),
  ADD COLUMN last_used_at timestamptz;

-- Every existing row is development seed data carrying read,write — which the
-- constraint below forbids for agents, and which we cannot honestly classify
-- as sessions. Drop them; everyone sets up fresh.
--
-- Tasks delegated to those agents point at the rows we are about to remove, so
-- unassign them first. The task table's assignee_matches_kind constraint means
-- the kind must be cleared in the same statement as the id.
UPDATE task
   SET assignee_kind = NULL,
       assignee_token_id = NULL,
       claimed_by = NULL,
       claim_expires_at = NULL,
       status = CASE WHEN status = 'in_progress' THEN 'open' ELSE status END
 WHERE assignee_token_id IS NOT NULL;

DELETE FROM credential;

-- The proposal guardrail, as a property of the schema rather than a rule a
-- future route could forget: an agent can never apply, only propose.
ALTER TABLE credential
  ADD CONSTRAINT agents_cannot_apply CHECK (
    kind <> 'agent' OR NOT (scopes && ARRAY['write', 'admin'])
  );

ALTER INDEX agent_token_owner_idx RENAME TO credential_owner_idx;
CREATE INDEX credential_kind_idx ON credential(kind);

CREATE TABLE setup_code (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  person_id   uuid NOT NULL REFERENCES person(id) ON DELETE CASCADE,
  code_hash   text NOT NULL UNIQUE,
  expires_at  timestamptz NOT NULL,
  used_at     timestamptz,
  created_at  timestamptz NOT NULL DEFAULT now()
);
-- At most one live code per person: issuing a new invite must void the old one.
CREATE UNIQUE INDEX setup_code_one_live_per_person
  ON setup_code(person_id) WHERE used_at IS NULL;
