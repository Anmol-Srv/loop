# Airtribe skills

Lifecycle order. Entry scripts are relative to `/Users/anmol/Drive/Airtribe/airtribe-control-plane/`.

| Skill | Load when | Entry script |
|---|---|---|
| `airtribe-triage` | Any new Airtribe request, before planning or creating a worktree. | `scripts/airtribe_triage.py classify --operation <class>` |
| `airtribe-planning` | Route is `code-change`; you need an evidence-backed impact map before editing. | — |
| `airtribe-task-lifecycle` | Driving a code task from intake through worktree, worker, and PR. | `scripts/airtribe_bridge.py candidates\|prepare\|launch\|status` |
| `airtribe-evidence-led-builds` | Writing the code: choosing error, retry, fallback, or queue behavior. | — |
| `airtribe-domain` | Task touches enrollment, cohort, unit, lead, opportunity, or role semantics. | — |
| `airtribe-data-changes` | Task touches schema, indexes, raw SQL, backfills, or concurrency. | — |
| `airtribe-async-integrations` | Task touches Bull, cron, Redis, provider APIs, or Sheets sync. | — |
| `airtribe-production-db-lookup` | Route is `prod-read-lookup` or `prod-repl-proposal`; a production fact is needed. | `scripts/run_prod_readonly_query.js` |
| `airtribe-production-operations` | A production record must change and a human-run REPL runbook is the artifact. | — |
| `airtribe-verification` | A candidate commit exists and needs independent gates, review, and PR. | `scripts/airtribe_bridge.py verify\|deliver` |
