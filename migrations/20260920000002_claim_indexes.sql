-- Lease expiry lookups (reaper sweeps, claimability checks on leased rows).
-- The claim lookup itself is already covered by task_claimable_idx from P0.
CREATE INDEX task_lease_idx ON task(claim_expires_at) WHERE claimed_by IS NOT NULL;
