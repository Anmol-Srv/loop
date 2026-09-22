"""Smallest checks that fail if the guard, wrapping, env mapping, or redaction break. No network."""
import json, os, sys, unittest
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import importlib
query = importlib.import_module("airtribe-prod-db.query")

class Guard(unittest.TestCase):
    def ok(self, s): return query.guard(s)
    def bad(self, s):
        with self.assertRaises(ValueError): query.guard(s)
    def test_select_passes(self): self.assertTrue(self.ok("select id from users where id = 1;"))
    def test_cte_passes(self): self.assertTrue(self.ok("with x as (select 1) select * from x"))
    def test_literal_containing_verb_passes(self): self.assertTrue(self.ok("select 1 where status = 'update pending'"))
    def test_comment_stripped(self): self.assertEqual(self.ok("select 1 -- drop table users"), "select 1")
    def test_rejects_write(self): self.bad("delete from users")
    def test_rejects_modifying_cte(self): self.bad("with d as (delete from users returning *) select * from d")
    def test_rejects_two_statements(self): self.bad("select 1; select 2")
    def test_rejects_meta(self): self.bad("\\copy users to '/tmp/x'")
    def test_rejects_set(self): self.bad("set role postgres")
    def test_rejects_select_into(self): self.bad("select * into t2 from t1")
    def test_offset_is_not_set(self): self.assertTrue(self.ok("select 1 offset 5"))

class Wrap(unittest.TestCase):
    def test_limit_added(self): self.assertIn("limit 21", query.wrap("select 1", 20))
    def test_explain_untouched(self): self.assertEqual(query.wrap("explain select 1", 5), "explain select 1")

class Env(unittest.TestCase):
    def test_url_to_env(self):
        e = query.pg_env("postgresql://ro%40x:p%40ss@db.example.com:25060/mycohort?sslmode=require")
        self.assertEqual((e["PGHOST"], e["PGPORT"], e["PGUSER"], e["PGPASSWORD"], e["PGDATABASE"]),
                         ("db.example.com", "25060", "ro@x", "p@ss", "mycohort"))
        self.assertIn("default_transaction_read_only=on", e["PGOPTIONS"])
        self.assertIn(e["PGSSLMODE"], ("require", "verify-full"))
    def test_sslrootcert_in_url(self):
        e = query.pg_env("postgresql://u:p@h/d?sslrootcert=/tmp/ca.crt")
        self.assertEqual((e["PGSSLMODE"], e["PGSSLROOTCERT"]), ("verify-full", "/tmp/ca.crt"))

class Redact(unittest.TestCase):
    def test_pii_columns(self):
        rows = query.redact_rows(["id", "email", "phone_number"], [{"id": "1", "email": "a@b", "phone_number": "9"}])
        self.assertEqual(rows, [{"id": "1", "email": "<redacted>", "phone_number": "<redacted>"}])

class Handler(unittest.TestCase):
    def test_missing_url(self):
        os.environ.pop(query.URL_ENV, None)
        self.assertIn("error", json.loads(query.prod_db_query({"sql": "select 1"})))
    def test_end_to_end_with_fake_psql(self):
        os.environ[query.URL_ENV] = "postgresql://ro:p@h:5432/d"
        query._role_cache.clear()
        calls = []
        def fake(stmts, env):
            calls.append(stmts)
            if "rolsuper" in stmts:
                return 0, "role,rolsuper,rolcreaterole,rolcreatedb,rolbypassrls,db_create,writable_relations\nro,f,f,f,f,f,0\n", ""
            return 0, "id,email\n1,a@b\n2,c@d\n3,e@f\n", ""
        query.run_psql, real = fake, query.run_psql
        try:
            out = json.loads(query.prod_db_query({"sql": "select id, email from users", "max_rows": 2}))
        finally:
            query.run_psql = real
        self.assertEqual(out["row_count"], 2); self.assertTrue(out["truncated"]); self.assertTrue(out["role_read_only"])
        self.assertEqual(out["rows"][0]["email"], "<redacted>")
        self.assertTrue(calls[-1].startswith("begin read only;")); self.assertTrue(calls[-1].rstrip().endswith("rollback;"))
        self.assertNotIn("p@h", json.dumps(out))

if __name__ == "__main__":
    unittest.main(verbosity=1)
