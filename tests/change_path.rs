use acp_server::models::change::{record, Actor, Op, TargetType};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

async fn seed_person(pool: &PgPool) -> Uuid {
    sqlx::query_scalar("INSERT INTO person (email, name) VALUES ($1, $2) RETURNING id")
        .bind("anmol@airtribe.live")
        .bind("Anmol")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn human_actor_records_an_applied_change(pool: PgPool) {
    let person_id = seed_person(&pool).await;
    let actor = Actor { label: "anmol@airtribe.live".into(), person_id: Some(person_id), can_apply: true };

    let mut tx = pool.begin().await.unwrap();
    let change_id = record(&mut tx, &actor, TargetType::Project, Uuid::new_v4(), Op::Create, json!({"key": "acp"}))
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let (state, applied_at): (String, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as("SELECT state, applied_at FROM change WHERE id = $1")
            .bind(change_id)
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(state, "applied");
    assert!(applied_at.is_some());
}

#[sqlx::test]
async fn agent_actor_records_a_pending_change(pool: PgPool) {
    let person_id = seed_person(&pool).await;
    let actor = Actor { label: "hermes".into(), person_id: Some(person_id), can_apply: false };

    let mut tx = pool.begin().await.unwrap();
    let change_id = record(&mut tx, &actor, TargetType::Task, Uuid::new_v4(), Op::Update, json!({"status": "done"}))
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let (state, applied_at, on_behalf_of): (String, Option<chrono::DateTime<chrono::Utc>>, Option<Uuid>) =
        sqlx::query_as("SELECT state, applied_at, on_behalf_of FROM change WHERE id = $1")
            .bind(change_id)
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(state, "pending", "an actor without apply rights must never write an applied change");
    assert!(applied_at.is_none());
    assert_eq!(on_behalf_of, Some(person_id));
}

#[sqlx::test]
async fn a_rolled_back_transaction_leaves_no_change(pool: PgPool) {
    let actor = Actor { label: "anmol@airtribe.live".into(), person_id: None, can_apply: true };

    let mut tx = pool.begin().await.unwrap();
    record(&mut tx, &actor, TargetType::Project, Uuid::new_v4(), Op::Create, json!({}))
        .await
        .unwrap();
    tx.rollback().await.unwrap();

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM change")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(count, 0);
}
