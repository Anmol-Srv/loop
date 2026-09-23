-- A year of use for a five-person team, for measuring the dashboard at scale.
--
--   psql "$URL" -v ON_ERROR_STOP=1 -f tests/fixtures/scale-seed.sql            base
--   psql "$URL" -v ON_ERROR_STOP=1 -v scale=5 -f tests/fixtures/scale-seed.sql stress
--
-- Base: 30 projects, 1,500 tasks, 3,000 notes, 2,000 artifacts, 10 labels.
-- `scale` multiplies everything but the people and the labels. Same five
-- people as render-seed.sql. Loaded by scripts/perf.sh into `acp_perf`.
--
-- Fixed ids, so tests/perf_frames.rs can open them:
--   project N  = 33333333-0000-0000-0000-<N, 12 digits>   (project 1 is the biggest)
--   task N     = 44444444-0000-0000-0000-<N, 12 digits>   (task 1 lives in project 1)
-- Task sizes are skewed: low-numbered projects hold more tasks, like a real
-- board where one or two projects carry most of the work.
\if :{?scale}
\else
  \set scale 1
\endif

BEGIN;

INSERT INTO person (id, email, name, role, department) VALUES
  ('55555555-0000-0000-0000-000000000001', 'anmol.srivastava@airtribe.live',   'Anmol',   'admin',   'backend'),
  ('55555555-0000-0000-0000-000000000002', 'dhaval@airtribe.live',             'Dhaval',  'manager', 'backend'),
  ('55555555-0000-0000-0000-000000000003', 'chinmay.kunkikar@airtribe.live',   'Chinmay', 'member',  'frontend'),
  ('55555555-0000-0000-0000-000000000004', 'evana.sajan@airtribe.live',        'Evana',   'member',  'design'),
  ('55555555-0000-0000-0000-000000000005', 'pratik.vishwakarma@airtribe.live', 'Pratik',  'member',  'design');

INSERT INTO label (name, colour)
SELECT n, (ARRAY['purple','green','blue','slate','red','orange','yellow','pink','teal','slate'])[i]
  FROM unnest(ARRAY['Q1','Q2','Q3','Q4','revenue','growth','platform','security','infra','ux'])
       WITH ORDINALITY AS l(n, i);

CREATE TEMP TABLE knobs AS
SELECT 30 * :scale AS projects, 1500 * :scale AS tasks,
       3000 * :scale AS notes, 2000 * :scale AS artifacts;

CREATE FUNCTION pg_temp.pid(n int) RETURNS uuid LANGUAGE sql IMMUTABLE AS
  $$ SELECT ('33333333-0000-0000-0000-' || lpad(n::text, 12, '0'))::uuid $$;
CREATE FUNCTION pg_temp.tid(n int) RETURNS uuid LANGUAGE sql IMMUTABLE AS
  $$ SELECT ('44444444-0000-0000-0000-' || lpad(n::text, 12, '0'))::uuid $$;
CREATE FUNCTION pg_temp.person(n int) RETURNS uuid LANGUAGE sql IMMUTABLE AS
  $$ SELECT ('55555555-0000-0000-0000-' || lpad((1 + n % 5)::text, 12, '0'))::uuid $$;

-- Projects over the year: most active, some paused, done or archived; a few
-- past their target date.
INSERT INTO project (id, key, name, description, status, priority, start_date, target_date, created_at, updated_at)
SELECT pg_temp.pid(i),
       'proj-' || i,
       (ARRAY['Lead rating','Checkout','LMS player','Admin audit','Onboarding','Payments','Search',
              'Notifications','Reports','Mobile web','Certificates','Referrals'])[1 + i % 12] || ' ' || i,
       repeat('What this project is for, who asked, and what done looks like. ', 1 + i % 4),
       CASE WHEN i % 10 IN (7) THEN 'paused' WHEN i % 10 IN (8) THEN 'done'
            WHEN i % 10 = 9 AND i > 10 THEN 'archived' ELSE 'active' END,
       i % 5,
       current_date - (365 - (i * 331) % 360),
       CASE WHEN i % 4 = 0 THEN NULL ELSE current_date - (365 - (i * 331) % 360) + 30 + (i * 17) % 120 END,
       now() - make_interval(days => 365 - (i * 331) % 360),
       now() - make_interval(days => i % 30)
  FROM knobs, generate_series(1, knobs.projects) i;

INSERT INTO project_label (project_id, label_id)
SELECT DISTINCT pg_temp.pid(i), l.id
  FROM knobs, generate_series(1, knobs.projects) i
  JOIN LATERAL (SELECT id FROM label ORDER BY name OFFSET (i * 3) % 10 LIMIT 1 + i % 3) l ON true;

-- Two phases each; tasks go in the first.
INSERT INTO phase (project_id, position, name, status)
SELECT pg_temp.pid(i), pos, (ARRAY['Build','Polish'])[pos + 1], CASE WHEN pos = 0 THEN 'active' ELSE 'planned' END
  FROM knobs, generate_series(1, knobs.projects) i, generate_series(0, 1) pos;

-- Tasks. Project is skewed toward low numbers (quadratic), task 1 in project 1.
-- Person n%6: 0..4 a teammate, 5 nobody. Status follows the assignee's track;
-- one in ten is blocked on an earlier task (the UPDATE below).
CREATE TEMP TABLE t_gen AS
SELECT i,
       CASE WHEN i = 1 THEN 1
            ELSE 1 + floor(k.projects * power(((i * 7919) % k.tasks)::float / k.tasks, 2))::int END AS proj,
       CASE WHEN i % 6 = 5 THEN NULL ELSE i % 6 END AS who,
       (i * 13) % 20 AS roll,
       365.0 * (k.tasks - i) / k.tasks AS age
  FROM knobs k, generate_series(1, k.tasks) i;

INSERT INTO task (id, phase_id, title, body, status, priority, assignee_kind, assignee_person_id,
                  blocked_by, done_at, manual_reason, created_at, updated_at)
SELECT pg_temp.tid(g.i),
       ph.id,
       (ARRAY['Wire','Fix','Design','Refactor','Spike','Ship','Test','Document','Review','Migrate'])[1 + g.i % 10]
         || ' ' || (ARRAY['the totals API','the rating screen','empty states','the retry budget','caption support',
                         'the permission matrix','drip scheduler','coupon validation','the sidebar','search ranking'])[1 + (g.i / 10) % 10]
         || ' #' || g.i,
       repeat('Context, acceptance criteria and a link or two. ', g.i % 8),
       s.status,
       g.i % 5,
       CASE WHEN g.who IS NULL THEN NULL ELSE 'human' END,
       CASE WHEN g.who IS NULL THEN NULL ELSE pg_temp.person(g.who) END,
       '{}',
       CASE WHEN s.terminal THEN now() - make_interval(secs => (g.age * 86400 * 0.8)::int) END,
       CASE WHEN s.terminal AND g.i % 7 = 0 THEN 'Done by hand; no PR.' END,
       now() - make_interval(secs => (g.age * 86400)::int),
       now() - make_interval(secs => (g.age * 86400 * 0.7)::int)
  FROM t_gen g
  JOIN phase ph ON ph.project_id = pg_temp.pid(g.proj) AND ph.position = 0
  CROSS JOIN LATERAL (
    SELECT st AS status,
           (dept = 'design' AND st = 'completed') OR (dept = 'eng' AND st = 'shipped') AS terminal
      FROM (SELECT CASE WHEN g.who IS NULL THEN 'none' WHEN g.who IN (3, 4) THEN 'design' ELSE 'eng' END AS dept) d,
      LATERAL (SELECT CASE d.dept
        -- older work is mostly finished; the newest tenth is in flight
        WHEN 'none'   THEN (ARRAY['open','open','open','dropped'])[1 + g.roll % 4]
        WHEN 'design' THEN CASE WHEN g.age > 36 AND g.roll < 14 THEN 'completed'
                           ELSE (ARRAY['open','in_progress','handoff','open','in_progress','dropped','in_progress'])[1 + g.roll % 7] END
        ELSE               CASE WHEN g.age > 36 AND g.roll < 14 THEN 'shipped'
                           ELSE (ARRAY['open','in_progress','completed','open','in_progress','dropped','in_progress'])[1 + g.roll % 7] END
        END AS st) x
  ) s;

-- One assigned task in ten is blocked on the task three before it.
UPDATE task t SET status = 'blocked', blocked_by = ARRAY[pg_temp.tid(g.i - 3)], done_at = NULL
  FROM t_gen g
 WHERE t.id = pg_temp.tid(g.i) AND g.i > 3 AND g.i % 9 = 4 AND t.assignee_person_id IS NOT NULL
   AND t.status NOT IN ('blocked');

INSERT INTO note (task_id, author_id, body, created_at)
SELECT pg_temp.tid(1 + (j * 31) % k.tasks),
       pg_temp.person(j),
       repeat('Status update, a question, or the answer to one. ', 1 + j % 5),
       now() - make_interval(secs => (k.notes - j) * 31536000.0 / k.notes)
  FROM knobs k, generate_series(1, k.notes) j;

-- Evidence on tasks; one in five on a project as a resource.
INSERT INTO artifact (parent_type, parent_id, kind, url, title, added_by, created_at)
SELECT CASE WHEN j % 5 = 0 THEN 'project' ELSE 'task' END,
       CASE WHEN j % 5 = 0 THEN pg_temp.pid(1 + (j * 7) % k.projects) ELSE pg_temp.tid(1 + (j * 17) % k.tasks) END,
       (ARRAY['pr','commit','figma','doc','link'])[1 + j % 5],
       'https://github.com/airtribe/mycohort-api/pull/' || (4000 + j),
       'Evidence ' || j,
       pg_temp.person(j),
       now() - make_interval(secs => (k.artifacts - j) * 31536000.0 / k.artifacts)
  FROM knobs k, generate_series(1, k.artifacts) j;

COMMIT;
ANALYZE;

SELECT (SELECT count(*) FROM project) AS projects, (SELECT count(*) FROM task) AS tasks,
       (SELECT count(*) FROM task WHERE blocked_by <> '{}') AS with_blockers,
       (SELECT count(*) FROM note) AS notes, (SELECT count(*) FROM artifact) AS artifacts,
       (SELECT count(*) FROM label) AS labels,
       (SELECT count(*) FROM task WHERE phase_id IN (SELECT id FROM phase WHERE project_id = '33333333-0000-0000-0000-000000000001')) AS project1_tasks;
SELECT status, count(*) FROM task GROUP BY 1 ORDER BY 2 DESC;
