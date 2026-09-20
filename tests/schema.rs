use sqlx::PgPool;

#[sqlx::test]
async fn all_nine_tables_exist(pool: PgPool) {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = 'public' ORDER BY table_name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    let names: Vec<String> = rows.into_iter().map(|r| r.0).collect();

    for expected in [
        "agent_token", "artifact", "change", "job", "person",
        "phase", "project", "run_log_line", "task",
    ] {
        assert!(names.contains(&expected.to_string()), "missing table {expected}");
    }
}

#[sqlx::test]
async fn task_requires_a_phase(pool: PgPool) {
    let result = sqlx::query(
        "INSERT INTO task (phase_id, title) VALUES (gen_random_uuid(), 'orphan')",
    )
    .execute(&pool)
    .await;

    assert!(result.is_err(), "a task with no real phase must be rejected");
}
