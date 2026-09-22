"""airtribe-prod-db: registers the prod_db_query tool."""
import os
import shutil

from . import query

SCHEMA = {
    "name": "prod_db_query",
    "description": (
        "Run ONE read-only SQL statement (SELECT, WITH, EXPLAIN, SHOW) against the Airtribe "
        "production Postgres through psql. Executes inside a READ ONLY transaction with a "
        f"{query.STATEMENT_TIMEOUT_MS // 1000}s statement timeout; rows are capped; columns that look "
        "like PII (email, phone, password, token, address, ...) are redacted unless redact=false. "
        "Use narrow predicates and select only the fields you need. Never for writes."
    ),
    "parameters": {
        "type": "object",
        "properties": {
            "sql": {"type": "string", "description": "A single SELECT/WITH/EXPLAIN/SHOW statement. No psql meta-commands, no multiple statements."},
            "max_rows": {"type": "integer", "description": f"Row cap, 1-{query.MAX_ROWS_HARD} (default {query.MAX_ROWS_DEFAULT})."},
            "redact": {"type": "boolean", "description": "Redact PII-looking columns (default true). Set false only when the task needs the value and the evidence will not be stored."},
        },
        "required": ["sql"],
    },
}


def _available() -> bool:
    return bool(os.environ.get(query.URL_ENV)) and shutil.which("psql") is not None


def register(ctx):
    ctx.register_tool(
        name="prod_db_query",
        toolset="airtribe_prod_db",
        schema=SCHEMA,
        handler=query.prod_db_query,
        check_fn=_available,
        requires_env=[query.URL_ENV],
        description="Read-only production Postgres lookup via psql",
        emoji="🔎",
    )
