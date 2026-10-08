-- Whether a task delivers a change (a PR to ship, a design to hand off). Off,
-- its track ends at `completed`: an investigation, a data fix, a question
-- answered. On for everything that exists, so nothing moves.
ALTER TABLE task ADD COLUMN needs_change boolean NOT NULL DEFAULT true;
