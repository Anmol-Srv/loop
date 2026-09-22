"""Guarded psql runner. Everything that makes the tool safe lives here, in one place."""
from __future__ import annotations

import csv
import io
import json
import os
import re
import subprocess
import time
from urllib.parse import parse_qs, unquote, urlparse

URL_ENV = "AIRTRIBE_PROD_READONLY_DATABASE_URL"
CA_ENV = "AIRTRIBE_PROD_DB_CA"
DEFAULT_CA = os.path.expanduser("~/.hermes/profiles/airtribe/certs/prod-db-ca.crt")
STATEMENT_TIMEOUT_MS = 15_000
LOCK_TIMEOUT_MS = 2_000
MAX_ROWS_DEFAULT = 20
MAX_ROWS_HARD = 200
MAX_CELL_CHARS = 500

_ALLOWED_START = re.compile(r"^\s*(select|with|explain|show|table|values)\b", re.I)
# Whole-word write/DDL/admin verbs anywhere in the statement (catches data-modifying CTEs too).
_FORBIDDEN = re.compile(
    r"\b(insert|update|delete|merge|drop|alter|truncate|create|grant|revoke|copy|call|do|lock|"
    r"vacuum|analyze|refresh|reindex|cluster|comment|security|reset|listen|notify|"
    r"pg_terminate_backend|pg_cancel_backend|pg_sleep|dblink|lo_import|lo_export)\b",
    re.I,
)
# `set` is fine only as `set local`/`set transaction`? No: forbid all `set` and `into`.
_FORBIDDEN_2 = re.compile(r"\bset\b|\binto\b", re.I)
_PII = re.compile(r"(email|phone|mobile|whatsapp|password|passwd|token|secret|otp|address|dob|birth|aadhaar|pan_|_pan\b|passport|ssn|card)", re.I)

_role_cache: dict[str, dict] = {}


def strip_comments(sql: str) -> str:
    sql = re.sub(r"/\*.*?\*/", " ", sql, flags=re.S)
    return re.sub(r"--[^\n]*", " ", sql)


def guard(sql: str) -> str:
    """Return the normalised statement or raise ValueError with a reason that names no data."""
    s = strip_comments(sql or "").strip().rstrip(";").strip()
    if not s:
        raise ValueError("empty sql")
    if "\\" in s:
        raise ValueError("psql meta-commands (backslash) are not allowed")
    if ";" in s:
        raise ValueError("exactly one statement per call")
    if not _ALLOWED_START.match(s):
        raise ValueError("statement must start with SELECT, WITH, EXPLAIN, SHOW, TABLE or VALUES")
    # Strip string literals so a column value like 'update' does not trip the verb check.
    bare = re.sub(r"'(?:[^']|'')*'", "''", s)
    hit = _FORBIDDEN.search(bare) or _FORBIDDEN_2.search(bare)
    if hit:
        raise ValueError(f"forbidden keyword: {hit.group(0).lower()}")
    return s


def wrap(sql: str, max_rows: int) -> str:
    """Cap rows for row-returning statements; EXPLAIN/SHOW run as-is."""
    if re.match(r"^\s*(explain|show)\b", sql, re.I):
        return sql
    return f"select * from (\n{sql}\n) _q limit {max_rows + 1}"


def pg_env(url: str) -> dict:
    """Turn the URL into libpq env vars so nothing lands on a command line or in `ps`."""
    p = urlparse(url)
    if p.scheme not in ("postgres", "postgresql"):
        raise ValueError("URL must be postgresql://")
    q = {k: v[0] for k, v in parse_qs(p.query).items()}
    env = {
        "PGHOST": p.hostname or "",
        "PGPORT": str(p.port or 5432),
        "PGUSER": unquote(p.username or ""),
        "PGPASSWORD": unquote(p.password or ""),
        "PGDATABASE": p.path.lstrip("/"),
        "PGCONNECT_TIMEOUT": "10",
        "PGAPPNAME": "hermes-airtribe-readonly",
        # Server-side belt: even a stray write in a bug cannot commit.
        "PGOPTIONS": (
            f"-c default_transaction_read_only=on -c statement_timeout={STATEMENT_TIMEOUT_MS} "
            f"-c lock_timeout={LOCK_TIMEOUT_MS} -c idle_in_transaction_session_timeout={STATEMENT_TIMEOUT_MS}"
        ),
    }
    ca = q.get("sslrootcert") or os.environ.get(CA_ENV) or (DEFAULT_CA if os.path.exists(DEFAULT_CA) else "")
    if ca:
        env["PGSSLMODE"] = q.get("sslmode", "verify-full")
        env["PGSSLROOTCERT"] = ca
    else:
        env["PGSSLMODE"] = q.get("sslmode", "require")  # ponytail: encrypted, chain unverified until the DO CA is installed
    return env


def run_psql(statements: str, env: dict) -> tuple[int, str, str]:
    """The only place psql is spawned. Returns (rc, stdout, stderr)."""
    full = {**os.environ, **env}
    p = subprocess.run(
        ["psql", "-X", "-v", "ON_ERROR_STOP=1", "--csv", "-q"],
        input=statements, capture_output=True, text=True, env=full, timeout=STATEMENT_TIMEOUT_MS / 1000 + 20,
    )
    return p.returncode, p.stdout, p.stderr


_ROLE_SQL = """
select current_user as role,
       r.rolsuper, r.rolcreaterole, r.rolcreatedb, r.rolbypassrls,
       has_database_privilege(current_database(), 'CREATE') as db_create,
       (select count(*) from pg_class c join pg_namespace n on n.oid = c.relnamespace
         where c.relkind in ('r','p','v','m') and n.nspname not in ('pg_catalog','information_schema')
           and (has_table_privilege(c.oid,'INSERT') or has_table_privilege(c.oid,'UPDATE') or has_table_privilege(c.oid,'DELETE'))
       ) as writable_relations
from pg_roles r where r.rolname = current_user
"""


def role_info(env: dict, runner=None) -> dict:
    runner = runner or run_psql
    key = env.get("PGHOST", "") + "/" + env.get("PGUSER", "")
    if key in _role_cache:
        return _role_cache[key]
    rc, out, err = runner(f"begin read only;{_ROLE_SQL};rollback;", env)
    if rc != 0:
        raise RuntimeError(sanitize(err, env))
    row = next(csv.DictReader(io.StringIO(out)))
    info = {
        "role": row["role"],
        "read_only": row["rolsuper"] == "f" and row["rolcreaterole"] == "f" and row["rolcreatedb"] == "f"
        and row["rolbypassrls"] == "f" and row["db_create"] == "f" and int(row["writable_relations"]) == 0,
        "writable_relations": int(row["writable_relations"]),
    }
    _role_cache[key] = info
    return info


def sanitize(text: str, env: dict) -> str:
    for k in ("PGPASSWORD", "PGHOST", "PGUSER"):
        if env.get(k):
            text = text.replace(env[k], f"<{k.lower()}>")
    return text.strip()[:800]


def redact_rows(columns: list[str], rows: list[dict]) -> list[dict]:
    hide = {c for c in columns if _PII.search(c)}
    return [{k: ("<redacted>" if k in hide else v) for k, v in r.items()} for r in rows]


def prod_db_query(args: dict, **kwargs) -> str:
    try:
        url = os.environ.get(URL_ENV)
        if not url:
            return json.dumps({"error": f"{URL_ENV} is not set in the profile environment"})
        max_rows = max(1, min(int(args.get("max_rows") or MAX_ROWS_DEFAULT), MAX_ROWS_HARD))
        sql = guard(args.get("sql", ""))
        env = pg_env(url)
        role = role_info(env)
        if not role["read_only"] and os.environ.get("AIRTRIBE_PROD_DB_TRUST_READONLY_TXN") != "1":
            return json.dumps({
                "error": "connected role has write capability; refusing. Use a true read-only role, "
                         "or set AIRTRIBE_PROD_DB_TRUST_READONLY_TXN=1 to rely on the READ ONLY transaction alone.",
                "role": role,
            })
        t0 = time.time()
        rc, out, err = run_psql(f"begin read only;\n{wrap(sql, max_rows)};\nrollback;", env)
        elapsed = int((time.time() - t0) * 1000)
        if rc != 0:
            return json.dumps({"error": sanitize(err, env), "elapsed_ms": elapsed})
        reader = csv.DictReader(io.StringIO(out))
        rows = [{k: (v if v is None or len(v) <= MAX_CELL_CHARS else v[:MAX_CELL_CHARS] + "…") for k, v in r.items()} for r in reader]
        columns = reader.fieldnames or []
        truncated = len(rows) > max_rows
        rows = rows[:max_rows]
        if args.get("redact", True):
            rows = redact_rows(columns, rows)
        return json.dumps({
            "columns": columns, "rows": rows, "row_count": len(rows), "truncated": truncated,
            "elapsed_ms": elapsed, "role": role["role"], "role_read_only": role["read_only"],
            "ssl_mode": env["PGSSLMODE"], "transaction": "READ ONLY, rolled back",
        }, default=str)
    except ValueError as e:
        return json.dumps({"error": f"rejected: {e}"})
    except subprocess.TimeoutExpired:
        return json.dumps({"error": "psql timed out"})
    except Exception as e:  # never raise into the agent loop
        return json.dumps({"error": str(e)[:800]})
