use acp_server::controllers::{people, token};
use acp_server::db::AppState;
use acp_server::models::{password, setup_code};
use sqlx::PgPool;

const EMAIL: &str = "anmol@airtribe.live";

async fn bootstrap(pool: &PgPool) -> (AppState, String) {
    let state = AppState { db: pool.clone() };
    let code = people::bootstrap_admin(&state, EMAIL, "Anmol", false)
        .await
        .unwrap();
    (state, code)
}

#[sqlx::test]
async fn an_agent_cannot_hold_write(pool: PgPool) {
    let (_state, _) = bootstrap(&pool).await;

    // Agents are minted with no scopes at all; bypass that, and the database
    // still refuses an agent that could apply.
    let person_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM person WHERE email = $1")
        .bind(EMAIL)
        .fetch_one(&pool)
        .await
        .unwrap();
    let agent_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO agent (owner_id, handle, name) VALUES ($1, 'sneaky', 'Sneaky') RETURNING id",
    )
    .bind(person_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let direct = sqlx::query(
        "INSERT INTO credential (kind, label, token_hash, owner_id, scopes, expires_at, agent_id)
         VALUES ('agent', 'sneaky', 'deadbeef', $1, ARRAY['read','write'], now() + interval '1 day', $2)",
    )
    .bind(person_id)
    .bind(agent_id)
    .execute(&pool)
    .await;
    assert!(
        direct.is_err(),
        "the constraint must reject an agent with write"
    );
}

#[sqlx::test]
async fn a_setup_code_is_single_use(pool: PgPool) {
    let (state, code) = bootstrap(&pool).await;

    people::set_password(&state, EMAIL, &code, "correct horse battery")
        .await
        .unwrap();

    let second = people::set_password(&state, EMAIL, &code, "another long password").await;
    assert!(second.is_err(), "a spent code must not work twice");
}

#[sqlx::test]
async fn reinviting_voids_the_previous_code(pool: PgPool) {
    let (state, first) = bootstrap(&pool).await;
    let second = people::invite(&state, EMAIL).await.unwrap();
    assert_ne!(first, second);

    assert!(
        people::set_password(&state, EMAIL, &first, "correct horse battery")
            .await
            .is_err(),
        "the old code must be dead"
    );
    let person = people::set_password(&state, EMAIL, &second, "correct horse battery")
        .await
        .unwrap();
    assert_eq!(person.role, "admin");
}

#[sqlx::test]
async fn an_expired_setup_code_fails(pool: PgPool) {
    let (state, code) = bootstrap(&pool).await;

    sqlx::query("UPDATE setup_code SET expires_at = now() - interval '1 hour'")
        .execute(&pool)
        .await
        .unwrap();

    assert!(
        people::set_password(&state, EMAIL, &code, "correct horse battery")
            .await
            .is_err()
    );
}

#[sqlx::test]
async fn revoking_a_person_ends_sessions_and_agents(pool: PgPool) {
    let (state, code) = bootstrap(&pool).await;
    people::set_password(&state, EMAIL, &code, "correct horse battery")
        .await
        .unwrap();

    let (session_raw, _) = token::mint_session(&state, EMAIL).await.unwrap();
    let owner = people::id_of(&state, EMAIL).await.unwrap();
    let agent_raw = acp_server::controllers::agent::create(
        &state, owner, "hermes", "", "hermes", true, false, "http://x",
    )
    .await
    .unwrap()
    .token;

    let revoked = people::revoke_person(&state, EMAIL).await.unwrap();
    assert_eq!(revoked, 2, "both the session and the agent must be revoked");

    assert!(acp_server::models::token::lookup(&pool, &session_raw)
        .await
        .unwrap()
        .is_none());
    assert!(acp_server::models::token::lookup(&pool, &agent_raw)
        .await
        .unwrap()
        .is_none());

    let deleted: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT deleted_at FROM person WHERE email = $1")
            .bind(EMAIL)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(deleted.is_some(), "the person must be marked deleted");
}

#[test]
fn passwords_verify_and_a_missing_account_costs_the_same() {
    let hash = password::hash("correct horse battery").unwrap();
    assert!(password::verify("correct horse battery", &hash));
    assert!(!password::verify("wrong horse battery", &hash));
    assert!(!password::verify_dummy("correct horse battery"));
}

#[test]
fn setup_codes_are_typable() {
    let code = setup_code::generate();
    assert_eq!(code.len(), 14, "three groups of four: {code}");
    assert!(
        !code.contains(['O', '0', 'I', '1']),
        "ambiguous character in {code}"
    );
    assert_ne!(code, setup_code::generate());
}
