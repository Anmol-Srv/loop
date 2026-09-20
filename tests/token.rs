use acp_server::controllers::token;
use acp_server::db::AppState;
use acp_server::models::token::{hash_token, lookup};
use sqlx::PgPool;

async fn seed_person(pool: &PgPool) -> uuid::Uuid {
    sqlx::query_scalar("INSERT INTO person (email, name) VALUES ($1, $2) RETURNING id")
        .bind("anmol@airtribe.live")
        .bind("Anmol")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[test]
fn hashing_is_stable_and_not_the_input() {
    let h = hash_token("secret");
    assert_eq!(h, hash_token("secret"));
    assert_ne!(h, "secret");
    assert_eq!(h.len(), 64);
}

#[sqlx::test]
async fn minted_token_resolves_and_is_stored_hashed(pool: PgPool) {
    seed_person(&pool).await;
    let state = AppState { db: pool.clone() };

    let (raw, row) = token::mint(&state, "laptop", "anmol@airtribe.live", vec!["read".into(), "write".into()], 30)
        .await
        .unwrap();

    assert_eq!(row.scopes, vec!["read".to_string(), "write".to_string()]);

    let stored: String = sqlx::query_scalar("SELECT token_hash FROM credential WHERE id = $1")
        .bind(row.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_ne!(stored, raw, "the raw token must never be stored");
    assert_eq!(stored, hash_token(&raw));

    let found = lookup(&pool, &raw).await.unwrap();
    assert_eq!(found.unwrap().id, row.id);
}

#[sqlx::test]
async fn revoked_and_expired_tokens_do_not_resolve(pool: PgPool) {
    seed_person(&pool).await;
    let state = AppState { db: pool.clone() };

    let (revoked_raw, revoked) = token::mint(&state, "old", "anmol@airtribe.live", vec!["read".into()], 30).await.unwrap();
    sqlx::query("UPDATE credential SET revoked_at = now() WHERE id = $1")
        .bind(revoked.id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(lookup(&pool, &revoked_raw).await.unwrap().is_none(), "revoked token must not resolve");

    let (expired_raw, expired) = token::mint(&state, "stale", "anmol@airtribe.live", vec!["read".into()], 30).await.unwrap();
    sqlx::query("UPDATE credential SET expires_at = now() - interval '1 day' WHERE id = $1")
        .bind(expired.id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(lookup(&pool, &expired_raw).await.unwrap().is_none(), "expired token must not resolve");
}

#[sqlx::test]
async fn minting_for_an_unknown_person_fails(pool: PgPool) {
    let state = AppState { db: pool };
    let result = token::mint(&state, "ghost", "nobody@airtribe.live", vec!["read".into()], 30).await;
    assert!(result.is_err());
}
