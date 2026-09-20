use sqlx::PgPool;
use uuid::Uuid;

/// Ten workers race for three claimable tasks. Every task must go to exactly one
/// worker: this is the property `FOR UPDATE SKIP LOCKED` buys us, and the one
/// thing no amount of clicking around can check.
#[sqlx::test]
async fn concurrent_claims_hand_out_each_task_exactly_once(pool: PgPool) {
    let state = acp_server::db::AppState { db: pool.clone() };

    sqlx::query("INSERT INTO person (email, name) VALUES ($1, $2)")
        .bind("anmol@airtribe.live")
        .bind("Anmol")
        .execute(&pool)
        .await
        .unwrap();
    let (_raw, token) = acp_server::controllers::token::mint(
        &state,
        "hermes",
        "anmol@airtribe.live",
        vec!["read".into(), "claim".into()],
        30,
    )
    .await
    .unwrap();
    let token_id = token.id;

    let project_id: Uuid =
        sqlx::query_scalar("INSERT INTO project (key, name) VALUES ('acp','ACP') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
    let phase_id: Uuid = sqlx::query_scalar(
        "INSERT INTO phase (project_id, name, position) VALUES ($1, 'Build', 1) RETURNING id",
    )
    .bind(project_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    for i in 0..3 {
        sqlx::query(
            "INSERT INTO task (phase_id, title, assignee_kind, assignee_token_id)
             VALUES ($1, $2, 'agent', $3)",
        )
        .bind(phase_id)
        .bind(format!("task {i}"))
        .bind(token_id)
        .execute(&pool)
        .await
        .unwrap();
    }

    let worker = |n: usize| {
        let state = state.clone();
        async move { acp_server::controllers::work::claim(&state, &format!("w{n}"), None).await }
    };

    let results = tokio::join!(
        worker(0), worker(1), worker(2), worker(3), worker(4),
        worker(5), worker(6), worker(7), worker(8), worker(9),
    );
    let results = [
        results.0, results.1, results.2, results.3, results.4,
        results.5, results.6, results.7, results.8, results.9,
    ];

    let mut ids: Vec<Uuid> = results
        .into_iter()
        .map(|r| r.expect("claim must not error"))
        .flatten()
        .map(|t| {
            assert_eq!(t.status, "in_progress");
            assert!(t.claimed_by.is_some());
            t.id
        })
        .collect();

    assert_eq!(ids.len(), 3, "three tasks, so exactly three winners");
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 3, "no task may be handed out twice");
}
