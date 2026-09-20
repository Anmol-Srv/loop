use std::path::Path;

use uuid::Uuid;

use serde::Serialize;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::task::DISCIPLINES;
use crate::models::{password, setup_code};

const DOMAIN: &str = "@airtribe.live";

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Person {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub role: String,
}

/// Addresses are lowercased and must be on the company domain. The team list
/// is the only source of accounts; there is no self-signup.
fn normalise(email: &str) -> AppResult<String> {
    let email = email.trim().to_lowercase();
    if !email.ends_with(DOMAIN) {
        return Err(AppError::BadRequest(format!(
            "'{email}' is not a {DOMAIN} address"
        )));
    }
    Ok(email)
}

async fn find(state: &AppState, email: &str) -> AppResult<Person> {
    sqlx::query_as::<_, Person>(
        "SELECT id, email, name, role FROM person WHERE email = $1 AND deleted_at IS NULL",
    )
    .bind(email)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("no person with email '{email}'")))
}

/// A person as the clients see them: who they are and what they can pick up.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PersonRow {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub role: String,
    pub disciplines: Vec<String>,
}

const PERSON_ROW_COLUMNS: &str = "id, email, name, role, disciplines";

/// The team, for assignment pickers and avatars.
pub async fn list(state: &AppState) -> AppResult<Vec<PersonRow>> {
    Ok(sqlx::query_as(&format!(
        "SELECT {PERSON_ROW_COLUMNS} FROM person WHERE deleted_at IS NULL ORDER BY name"
    ))
    .fetch_all(&state.db)
    .await?)
}

/// Set your own disciplines. No `change` row: `change.target_type` covers the
/// board (project, phase, task, artifact) and a person is not on the board —
/// this is a profile setting, like a display name.
pub async fn set_disciplines(
    state: &AppState,
    person_id: Uuid,
    disciplines: Vec<String>,
) -> AppResult<PersonRow> {
    for d in &disciplines {
        if !DISCIPLINES.contains(&d.as_str()) {
            return Err(AppError::BadRequest(format!(
                "unknown discipline '{d}'; expected one of {}",
                DISCIPLINES.join(", ")
            )));
        }
    }

    sqlx::query_as(&format!(
        "UPDATE person SET disciplines = $2, updated_at = now()
          WHERE id = $1 AND deleted_at IS NULL RETURNING {PERSON_ROW_COLUMNS}"
    ))
    .bind(person_id)
    .bind(&disciplines)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("person not found".into()))
}

/// Read `email,Name` (or bare `email`) lines and create members. Existing
/// people are left exactly as they are; a bad address fails that line only.
pub async fn seed_team(state: &AppState, path: &Path) -> AppResult<Vec<String>> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| AppError::BadRequest(format!("cannot read {}: {e}", path.display())))?;

    let mut report = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let (raw_email, raw_name) = match line.split_once(',') {
            Some((e, n)) => (e, n.trim().to_string()),
            None => (line, String::new()),
        };

        let email = match normalise(raw_email) {
            Ok(e) => e,
            Err(e) => {
                report.push(format!("skipped {raw_email}: {e}"));
                continue;
            }
        };

        // Fall back to the local part when the file gives no name.
        let name = if raw_name.is_empty() {
            email.trim_end_matches(DOMAIN).to_string()
        } else {
            raw_name
        };

        let inserted = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO person (email, name, role) VALUES ($1, $2, 'member')
             ON CONFLICT (email) DO NOTHING RETURNING id",
        )
        .bind(&email)
        .bind(&name)
        .fetch_optional(&state.db)
        .await?;

        report.push(match inserted {
            Some(_) => format!("created {email}"),
            None => format!("exists  {email}"),
        });
    }

    Ok(report)
}

/// Create or promote an admin and hand back a setup code. This is the only way
/// to make the first admin, so it talks to the database directly — but an
/// accidental second run must not quietly grant administrative access.
pub async fn bootstrap_admin(
    state: &AppState,
    email: &str,
    name: &str,
    force: bool,
) -> AppResult<String> {
    let email = normalise(email)?;

    let mut tx = state.db.begin().await?;

    if !force {
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT email FROM person WHERE role = 'admin' AND deleted_at IS NULL LIMIT 1",
        )
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(existing) = existing {
            return Err(AppError::Conflict(format!(
                "'{existing}' is already an admin; re-run with --force to add another"
            )));
        }
    }

    let person_id: Uuid = sqlx::query_scalar(
        "INSERT INTO person (email, name, role) VALUES ($1, $2, 'admin')
         ON CONFLICT (email) DO UPDATE
           SET role = 'admin', deleted_at = NULL, updated_at = now()
         RETURNING id",
    )
    .bind(&email)
    .bind(name)
    .fetch_one(&mut *tx)
    .await?;

    let code = setup_code::issue(&mut tx, person_id).await?;
    tx.commit().await?;

    Ok(code)
}

/// Issue a setup code, voiding any code the person still has outstanding.
/// A forgotten password is this command again.
pub async fn invite(state: &AppState, email: &str) -> AppResult<String> {
    let email = normalise(email)?;
    let person = find(state, &email).await?;

    let mut tx = state.db.begin().await?;
    let code = setup_code::issue(&mut tx, person.id).await?;
    tx.commit().await?;

    Ok(code)
}

/// Offboarding: every session and every agent they own, plus the person, in
/// one transaction. Returns how many credentials were revoked.
pub async fn revoke_person(state: &AppState, email: &str) -> AppResult<u64> {
    let email = normalise(email)?;
    let person = find(state, &email).await?;

    let mut tx = state.db.begin().await?;

    let revoked = sqlx::query(
        "UPDATE credential SET revoked_at = now()
         WHERE owner_id = $1 AND revoked_at IS NULL",
    )
    .bind(person.id)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    sqlx::query("UPDATE person SET deleted_at = now(), updated_at = now() WHERE id = $1")
        .bind(person.id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(revoked)
}

/// Spend a setup code and set the password. The caller mints the session, so
/// this returns the person rather than a credential.
pub async fn set_password(
    state: &AppState,
    email: &str,
    code: &str,
    plain: &str,
) -> AppResult<Person> {
    let email = normalise(email)?;
    password::validate(plain, &email)?;
    let hash = password::hash(plain)?;

    let mut tx = state.db.begin().await?;

    let person_id = setup_code::redeem(&mut tx, code)
        .await?
        .ok_or_else(|| AppError::BadRequest("that setup code is not valid".into()))?;

    let person = sqlx::query_as::<_, Person>(
        "UPDATE person
            SET password_hash = $1, password_set_at = now(),
                failed_attempts = 0, locked_until = NULL, updated_at = now()
          WHERE id = $2 AND email = $3 AND deleted_at IS NULL
          RETURNING id, email, name, role",
    )
    .bind(&hash)
    .bind(person_id)
    .bind(&email)
    .fetch_optional(&mut *tx)
    .await?
    // The code is real but belongs to someone else: same message, so a code
    // cannot be used to probe which address it was issued to.
    .ok_or_else(|| AppError::BadRequest("that setup code is not valid".into()))?;

    tx.commit().await?;
    Ok(person)
}
