-- A task with no phase is standalone: "No project". Created directly, or
-- moved out of a project; moved back in, it lands in the project's first phase.
ALTER TABLE task ALTER COLUMN phase_id DROP NOT NULL;
