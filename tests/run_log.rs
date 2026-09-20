use sqlx::PgPool;
use uuid::Uuid;

/// A leased task owned by `hermes`, so `append` passes its lease check.
async fn leased_task(pool: &PgPool) -> Uuid {
    let project_id: Uuid =
        sqlx::query_scalar("INSERT INTO project (key, name) VALUES ('acp','ACP') RETURNING id")
            .fetch_one(pool).await.unwrap();
    let phase_id: Uuid = sqlx::query_scalar(
        "INSERT INTO phase (project_id, name, position) VALUES ($1, 'Build', 1) RETURNING id")
        .bind(project_id).fetch_one(pool).await.unwrap();
    sqlx::query_scalar(
        "INSERT INTO task (phase_id, title, claimed_by, claim_expires_at)
         VALUES ($1, 'migrate report', 'hermes', now() + interval '5 minutes') RETURNING id")
        .bind(phase_id).fetch_one(pool).await.unwrap()
}

/// The only run-log test that earns its keep: a `seq` race is invisible until
/// it corrupts a log. Ordering, paging and the 403 are covered by reading the
/// controller.
#[sqlx::test]
async fn concurrent_appends_allocate_distinct_seq(pool: PgPool) {
    let task_id = leased_task(&pool).await;
    let state = acp_server::db::AppState { db: pool };

    // Separate tasks, not `join!` on one future: the writers must really be
    // inside their transactions at the same time for the race to be possible.
    // Eight of them, not two — with only two the second transaction reliably
    // starts after the first has committed, and a `seq` allocation with no
    // lock at all passes. Eight collides on the unlocked version every run.
    let writers: Vec<_> = ["a", "b", "c", "d", "e", "f", "g", "h"]
        .iter()
        .map(|tag| {
            let state = state.clone();
            let lines: Vec<String> = (1..=5).map(|i| format!("{tag}-{i}")).collect();
            tokio::spawn(async move {
                acp_server::controllers::run_log::append(&state, "hermes", task_id, lines).await
            })
        })
        .collect();

    for w in writers {
        w.await.unwrap().expect("concurrent append must not hit the unique constraint");
    }

    let lines = acp_server::controllers::run_log::read(&state, task_id, 0).await.unwrap();
    assert_eq!(lines.len(), 40);

    let mut seqs: Vec<i64> = lines.iter().map(|l| l.seq).collect();
    assert_eq!(seqs, (1..=40).collect::<Vec<i64>>(), "seq must be dense and ordered");
    seqs.dedup();
    assert_eq!(seqs.len(), 40);
}
