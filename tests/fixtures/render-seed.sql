-- A realistic slice of Airtribe engineering, for rendering the app offscreen.
--
-- Loaded into its own database (`acp_render`) by scripts/render-pages.sh, never
-- into the working one. Every state a task can be in appears at least once,
-- one project is overdue, one is paused and one is finished, and completions
-- are spread over two weeks so the weekly chart has something to compare.
-- Projects and the tasks the harness opens have fixed ids so the harness can
-- navigate to them.
DO $$
DECLARE
  anmol   uuid; dhaval uuid; chinmay uuid; evana uuid; pratik uuid;
  lr uuid := '11111111-0000-0000-0000-000000000001';
  co uuid := '11111111-0000-0000-0000-000000000002';
  lms uuid := '11111111-0000-0000-0000-000000000003';
  adm uuid := '11111111-0000-0000-0000-000000000004';
  onb uuid := '11111111-0000-0000-0000-000000000005';
  ph_lr uuid; ph_co uuid; ph_lms uuid; ph_adm uuid; ph_onb uuid;
  t_eng uuid := '22222222-0000-0000-0000-000000000001';
  t_des uuid := '22222222-0000-0000-0000-000000000002';
  t_ship uuid := '22222222-0000-0000-0000-000000000003';
  t_totals uuid := '22222222-0000-0000-0000-000000000004';
  t_screens uuid := '22222222-0000-0000-0000-000000000005';
  q4 uuid; revenue uuid; growth uuid; platform uuid; security uuid;
BEGIN
  INSERT INTO person (email, name, role, department) VALUES
    ('anmol.srivastava@airtribe.live', 'Anmol', 'admin', 'backend') RETURNING id INTO anmol;
  INSERT INTO person (email, name, role, department) VALUES
    ('dhaval@airtribe.live', 'Dhaval', 'manager', 'backend') RETURNING id INTO dhaval;
  INSERT INTO person (email, name, role, department) VALUES
    ('chinmay.kunkikar@airtribe.live', 'Chinmay', 'member', 'frontend') RETURNING id INTO chinmay;
  INSERT INTO person (email, name, role, department) VALUES
    ('evana.sajan@airtribe.live', 'Evana', 'member', 'design') RETURNING id INTO evana;
  INSERT INTO person (email, name, role, department) VALUES
    ('pratik.vishwakarma@airtribe.live', 'Pratik', 'member', 'design') RETURNING id INTO pratik;

  INSERT INTO label (name, colour) VALUES ('Q4', 'purple') RETURNING id INTO q4;
  INSERT INTO label (name, colour) VALUES ('revenue', 'green') RETURNING id INTO revenue;
  INSERT INTO label (name, colour) VALUES ('growth', 'blue') RETURNING id INTO growth;
  INSERT INTO label (name, colour) VALUES ('platform', 'slate') RETURNING id INTO platform;
  INSERT INTO label (name, colour) VALUES ('security', 'red') RETURNING id INTO security;

  INSERT INTO project (id, key, name, description, status, priority, start_date, target_date, created_at) VALUES
    (lr, 'lead-rating-v2', 'Lead Rating v2',
     E'Backend-engineering leads on the weighted-rubric path get their role bucket and all five rating dimensions from Jev, while GPT keeps free-text extraction only.\n\nA failed or low-confidence Jev response persists nothing, leaving the lead retryable exactly as a GPT failure already did.',
     'active', 1, current_date - 10, current_date + 18, now() - interval '10 days'),
    (co, 'checkout-redesign', 'Checkout redesign',
     'One-page checkout with saved payment methods and coupon validation before the pay step.',
     'active', 0, current_date - 30, current_date - 5, now() - interval '30 days'),
    (lms, 'lms-video-player', 'LMS video player',
     'Replace the embedded player with HLS streaming, resume-from-timestamp and captions.',
     'active', 2, current_date - 3, current_date + 5, now() - interval '3 days'),
    (adm, 'admin-permissions-audit', 'Admin permissions audit',
     'Every admin route mapped to the permission that guards it.',
     'paused', 3, NULL, NULL, now() - interval '21 days'),
    (onb, 'onboarding-emails', 'Onboarding emails',
     'Welcome series and drip scheduler for new learners.',
     'done', 2, current_date - 40, current_date - 12, now() - interval '40 days');

  INSERT INTO project_label VALUES (lr, q4), (lr, revenue), (co, revenue), (co, growth),
    (lms, platform), (adm, security), (onb, growth);

  INSERT INTO phase (project_id, position, name, status) VALUES (lr, 0, 'Work', 'active') RETURNING id INTO ph_lr;
  INSERT INTO phase (project_id, position, name, status) VALUES (co, 0, 'Work', 'active') RETURNING id INTO ph_co;
  INSERT INTO phase (project_id, position, name, status) VALUES (lms, 0, 'Work', 'active') RETURNING id INTO ph_lms;
  INSERT INTO phase (project_id, position, name, status) VALUES (adm, 0, 'Work', 'active') RETURNING id INTO ph_adm;
  INSERT INTO phase (project_id, position, name, status) VALUES (onb, 0, 'Work', 'active') RETURNING id INTO ph_onb;

  -- Lead Rating v2
  INSERT INTO task (id, phase_id, title, body, status, priority, assignee_kind, assignee_person_id, created_at, updated_at) VALUES
    (t_eng, ph_lr, 'Jev client and retry budget',
     E'New api/services/jev.js boundary: client config, a bounded per-attempt timeout and retry budget, response validation.\n\nReturns null on every failure rather than throwing, so callers can tell "no answer" from a real one.',
     'in_progress', 1, 'human', anmol, now() - interval '8 days', now() - interval '1 day');
  INSERT INTO task (id, phase_id, title, body, status, priority, assignee_kind, assignee_person_id, created_at, updated_at) VALUES
    (t_screens, ph_lr, 'Rating screens', 'Five dimensions, role bucket, low-confidence state.',
     'handoff', 2, 'human', evana, now() - interval '7 days', now() - interval '2 days');
  INSERT INTO task (phase_id, title, body, status, priority, assignee_kind, assignee_person_id, blocked_by, created_at, updated_at) VALUES
    (ph_lr, 'Role bucket taxonomy', 'Drop in the product-supplied taxonomy once it lands.', 'blocked', 1,
     'human', dhaval, ARRAY[t_screens], now() - interval '6 days', now() - interval '3 days'),
    (ph_lr, 'Wire the new rating fields', '', 'open', 2, 'human', chinmay, '{}', now() - interval '5 days', now() - interval '5 days'),
    (ph_lr, 'Regression tests for the GPT path', 'Product, other subdomains and Instagram leads stay on GPT.', 'completed', 2,
     'human', anmol, '{}', now() - interval '6 days', now() - interval '1 day');
  INSERT INTO task (id, phase_id, title, body, status, priority, assignee_kind, assignee_person_id, done_at, created_at, updated_at) VALUES
    (t_ship, ph_lr, 'Drop the GPT role-bucket question',
     'Its question and schema field are removed for weighted-rubric leads, so there is no stale second opinion to fall back to.',
     'shipped', 1, 'human', dhaval, now() - interval '2 days', now() - interval '9 days', now() - interval '2 days');

  -- Checkout redesign (overdue)
  INSERT INTO task (id, phase_id, title, body, status, priority, assignee_kind, assignee_person_id, created_at, updated_at) VALUES
    (t_totals, ph_co, 'Cart totals API', 'Server-side totals with tax and coupon applied, one call.',
     'in_progress', 0, 'human', anmol, now() - interval '12 days', now() - interval '1 day');
  INSERT INTO task (id, phase_id, title, body, status, priority, assignee_kind, assignee_person_id, done_at, created_at, updated_at) VALUES
    (t_des, ph_co, 'Cart and checkout flows', 'Every step of the one-page checkout, mobile first.',
     'completed', 1, 'human', pratik, now() - interval '9 days', now() - interval '25 days', now() - interval '9 days');
  INSERT INTO task (phase_id, title, body, status, priority, assignee_kind, assignee_person_id, blocked_by, created_at, updated_at) VALUES
    (ph_co, 'Payment picker', 'Saved cards first, then UPI and netbanking.', 'blocked', 0,
     'human', chinmay, ARRAY[t_totals], now() - interval '11 days', now() - interval '4 days');
  INSERT INTO task (phase_id, title, body, status, priority, assignee_kind, assignee_person_id, created_at, updated_at) VALUES
    (ph_co, 'Error and empty states', 'Declined card, expired coupon, empty cart.', 'in_progress', 1,
     'human', evana, now() - interval '10 days', now() - interval '2 days'),
    (ph_co, 'Order summary polish', '', 'open', 2, 'human', chinmay, now() - interval '4 days', now() - interval '4 days'),
    (ph_co, 'Remove the legacy cart', 'Superseded by the redesign.', 'dropped', 3,
     'human', anmol, now() - interval '20 days', now() - interval '15 days');
  INSERT INTO task (phase_id, title, body, status, priority, assignee_kind, assignee_person_id, done_at, created_at, updated_at) VALUES
    (ph_co, 'Coupon validation', 'Validate before the pay step, not after.', 'shipped', 1,
     'human', dhaval, now() - interval '4 days', now() - interval '14 days', now() - interval '4 days');

  -- LMS video player
  INSERT INTO task (phase_id, title, body, status, priority, assignee_kind, assignee_person_id, created_at, updated_at) VALUES
    (ph_lms, 'Player controls', 'Speed, captions toggle, chapter markers.', 'in_progress', 2,
     'human', pratik, now() - interval '3 days', now() - interval '1 day'),
    (ph_lms, 'HLS streaming spike', 'Can the CDN serve HLS without re-encoding the back catalogue?', 'open', 1,
     'human', anmol, now() - interval '2 days', now() - interval '2 days'),
    (ph_lms, 'Resume from timestamp', '', 'open', 2, 'human', chinmay, now() - interval '2 days', now() - interval '2 days');
  INSERT INTO task (phase_id, title, body, status, priority, created_at, updated_at) VALUES
    (ph_lms, 'Captions and transcripts', 'Nobody holds this yet.', 'open', 3, now() - interval '1 day', now() - interval '1 day');

  -- Admin permissions audit (paused)
  INSERT INTO task (phase_id, title, body, status, priority, assignee_kind, assignee_person_id, manual_reason, created_at, updated_at) VALUES
    (ph_adm, 'Inventory the admin routes', '', 'completed', 3, 'human', dhaval,
     'Listed from the router by hand; no code change.', now() - interval '20 days', now() - interval '18 days');
  INSERT INTO task (phase_id, title, body, status, priority, created_at, updated_at) VALUES
    (ph_adm, 'Permission matrix doc', '', 'open', 3, now() - interval '19 days', now() - interval '19 days');

  -- Onboarding emails (done)
  INSERT INTO task (phase_id, title, body, status, priority, assignee_kind, assignee_person_id, done_at, created_at, updated_at) VALUES
    (ph_onb, 'Welcome email templates', '', 'completed', 2, 'human', evana,
     now() - interval '14 days', now() - interval '38 days', now() - interval '14 days'),
    (ph_onb, 'Drip scheduler', 'BullMQ job, one per learner, cancelled on unsubscribe.', 'shipped', 2, 'human', dhaval,
     now() - interval '13 days', now() - interval '35 days', now() - interval '13 days'),
    (ph_onb, 'Unsubscribe link fix', '', 'shipped', 1, 'human', chinmay,
     now() - interval '1 day', now() - interval '3 days', now() - interval '1 day');

  -- Evidence
  INSERT INTO artifact (parent_type, parent_id, kind, url, title, added_by, created_at) VALUES
    ('task', t_eng, 'pr', 'https://github.com/airtribe/mycohort-api/pull/4821', 'Jev service boundary', anmol, now() - interval '2 days'),
    ('task', t_screens, 'figma', 'https://www.figma.com/file/aR7x/Lead-rating', 'Rating screens v3', evana, now() - interval '2 days'),
    ('task', t_ship, 'pr', 'https://github.com/airtribe/mycohort-api/pull/4812', 'Drop GPT role bucket for weighted-rubric leads', dhaval, now() - interval '3 days'),
    ('task', t_ship, 'commit', '9f3c2ab', 'Remove getRoleBucket prompt field', dhaval, now() - interval '2 days'),
    ('task', t_ship, 'doc', 'https://www.notion.so/airtribe/Lead-rating-rubric', 'Rating rubric', anmol, now() - interval '9 days'),
    ('task', t_des, 'figma', 'https://www.figma.com/file/cK2q/Checkout', 'Checkout flows', pratik, now() - interval '10 days'),
    -- Project-level resources, so the project page's Resources section is
    -- seen with something in it.
    ('project', lr, 'doc', 'https://www.notion.so/airtribe/Lead-rating-v2-spec', 'Lead rating v2 spec', anmol, now() - interval '10 days'),
    ('project', lr, 'figma', 'https://www.figma.com/file/aR7x/Lead-rating', 'Lead rating screens', evana, now() - interval '7 days'),
    ('project', co, 'link', 'https://airtribe.slack.com/archives/C0CHECKOUT', 'Checkout channel', dhaval, now() - interval '28 days');

  INSERT INTO note (task_id, author_id, body, created_at) VALUES
    (t_eng, dhaval, 'Is the retry budget per attempt or for the whole call?', now() - interval '2 days'),
    (t_eng, anmol, E'Per attempt: three tries at two seconds each, with the whole call capped at eight.\n\nAnything slower and the lead just stays retryable.', now() - interval '1 day'),
    (t_ship, anmol, 'Shipped with the 22 Sep deploy. Watching the retry rate.', now() - interval '2 days'),
    (t_totals, chinmay, 'The payment picker is waiting on this — can the response include the coupon breakdown?', now() - interval '4 days');
END $$;
