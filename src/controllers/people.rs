use std::path::Path;

use uuid::Uuid;

use serde::Serialize;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use serde_json::json;

use crate::models::change::{record, Actor, Op, TargetType};
use crate::models::task::{settle, DEPARTMENTS};
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
    /// The one department this person belongs to. A task they hold takes its
    /// discipline from here.
    pub department: String,
}

const PERSON_ROW_COLUMNS: &str = "id, email, name, role, department";

/// The team, for assignment pickers and avatars.
pub async fn list(state: &AppState) -> AppResult<Vec<PersonRow>> {
    Ok(sqlx::query_as(&format!(
        "SELECT {PERSON_ROW_COLUMNS} FROM person WHERE deleted_at IS NULL ORDER BY name"
    ))
    .fetch_all(&state.db)
    .await?)
}

/// The roles a person can hold. `manager` sees the same board as a member;
/// the database has allowed it since departments landed and the API now does
/// too.
pub const ROLES: [&str; 3] = ["member", "manager", "admin"];

/// Move a person to another department — an admin action, because it moves
/// every task they hold onto another track.
///
/// A task's track is read off its assignee, so a department change is a
/// reassignment of all their work at once, and it follows the same rule:
/// `task::settle` puts a status the new track lacks back to `open` and
/// recomputes `done_at`. Only in-flight work moves; a task already finished
/// stays finished as it was, since rewriting a shipped task to `open` would
/// erase the fact that it shipped. Each task that moves gets a `change` row,
/// because the board changed even though nobody touched it.
pub async fn set_department(
    state: &AppState,
    actor: &Actor,
    person_id: Uuid,
    department: &str,
) -> AppResult<PersonRow> {
    if !DEPARTMENTS.contains(&department) {
        return Err(AppError::BadRequest(format!(
            "unknown department '{department}'; expected one of {}",
            DEPARTMENTS.join(", ")
        )));
    }

    let mut tx = state.db.begin().await?;
    let person: PersonRow = sqlx::query_as(&format!(
        "UPDATE person SET department = $2, updated_at = now()
          WHERE id = $1 AND deleted_at IS NULL RETURNING {PERSON_ROW_COLUMNS}"
    ))
    .bind(person_id)
    .bind(department)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound("person not found".into()))?;

    // ponytail: one UPDATE per task that moves. A person holds tens of tasks.
    let held: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, status FROM task
          WHERE assignee_person_id = $1 AND done_at IS NULL FOR UPDATE",
    )
    .bind(person_id)
    .fetch_all(&mut *tx)
    .await?;
    for (id, status) in held {
        let (next, finished) = settle(Some(department), &status);
        if next == status && !finished {
            continue;
        }
        sqlx::query(
            "UPDATE task SET status = $2, updated_at = now(),
                    done_at = CASE WHEN $3 THEN now() ELSE NULL END
              WHERE id = $1",
        )
        .bind(id)
        .bind(&next)
        .bind(finished)
        .execute(&mut *tx)
        .await?;
        let patch = json!({ "status": next, "department": department });
        record(&mut tx, actor, TargetType::Task, id, Op::Update, patch).await?;
    }

    tx.commit().await?;
    Ok(person)
}

/// Change a person's role. Scopes are baked into a credential when it is
/// minted, so their sessions end with the change — that is what makes a
/// demotion real. Agents are left alone; they never carry `admin`.
///
/// Refuses to leave the system with no administrator, whoever asks: the last
/// admin is the last admin.
pub async fn set_role(state: &AppState, person_id: Uuid, role: &str) -> AppResult<(PersonRow, u64)> {
    if !ROLES.contains(&role) {
        return Err(AppError::BadRequest(format!(
            "unknown role '{role}'; expected one of {}",
            ROLES.join(", ")
        )));
    }

    let mut tx = state.db.begin().await?;
    // Every admin row is locked, so two demotions at once cannot each see the
    // other as the admin who remains.
    let admins: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM person WHERE role = 'admin' AND deleted_at IS NULL FOR UPDATE",
    )
    .fetch_all(&mut *tx)
    .await?;
    if role != "admin" && admins == [person_id] {
        return Err(AppError::Conflict(
            "this is the only admin; promote someone else before demoting them".into(),
        ));
    }

    let person: PersonRow = sqlx::query_as(&format!(
        "UPDATE person SET role = $2, updated_at = now()
          WHERE id = $1 AND deleted_at IS NULL RETURNING {PERSON_ROW_COLUMNS}"
    ))
    .bind(person_id)
    .bind(role)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound("person not found".into()))?;

    let ended = end_sessions(&mut tx, person_id).await?;
    tx.commit().await?;
    Ok((person, ended))
}

/// A person's id from their address, for the CLI and the email-keyed admin
/// routes.
pub async fn id_of(state: &AppState, email: &str) -> AppResult<Uuid> {
    Ok(find(state, &email.trim().to_lowercase()).await?.id)
}

/// Add one member — `acp-admin add-person`. The same address rules as
/// `seed_team`, so a typo'd or off-domain address cannot become an account
/// nobody can sign in as. True when the person was created, false when they
/// already existed.
pub async fn add_person(state: &AppState, email: &str, name: &str) -> AppResult<bool> {
    let email = normalise(email)?;
    let name = match name.trim() {
        "" => email.trim_end_matches(DOMAIN).to_string(),
        n => n.to_string(),
    };
    Ok(sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO person (email, name, role) VALUES ($1, $2, 'member')
         ON CONFLICT (email) DO NOTHING RETURNING id",
    )
    .bind(&email)
    .bind(&name)
    .fetch_optional(&state.db)
    .await?
    .is_some())
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

        let email = raw_email.trim().to_lowercase();
        report.push(match add_person(state, raw_email, &raw_name).await {
            Ok(true) => format!("created {email}"),
            Ok(false) => format!("exists  {email}"),
            // A bad address fails its own line, not the file.
            Err(AppError::BadRequest(e)) => format!("skipped {raw_email}: {e}"),
            Err(e) => return Err(e),
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

    end_sessions(&mut tx, person.id).await?;
    tx.commit().await?;
    Ok(person)
}

/// Set a password without a setup code — `acp-admin set-password`. Held to the
/// same rules as the app: a shared deployment is exactly where a short
/// break-glass password would outlive the emergency.
pub async fn set_password_directly(state: &AppState, email: &str, plain: &str) -> AppResult<()> {
    let email = normalise(email)?;
    password::validate(plain, &email)?;
    let hash = password::hash(plain)?;

    let mut tx = state.db.begin().await?;
    let person_id: Uuid = sqlx::query_scalar(
        "UPDATE person
            SET password_hash = $2, password_set_at = now(),
                failed_attempts = 0, locked_until = NULL, updated_at = now()
          WHERE email = $1 AND deleted_at IS NULL
          RETURNING id",
    )
    .bind(&email)
    .bind(&hash)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("no person with email '{email}'")))?;

    end_sessions(&mut tx, person_id).await?;
    tx.commit().await?;
    Ok(())
}

/// A new password is usually a response to an old one leaking, so every
/// session signed in under the old one ends with it. Agents are left alone:
/// they were minted deliberately and never held the password.
async fn end_sessions(tx: &mut sqlx::PgConnection, person_id: Uuid) -> AppResult<u64> {
    Ok(sqlx::query(
        "UPDATE credential SET revoked_at = now()
          WHERE owner_id = $1 AND kind = 'session' AND revoked_at IS NULL",
    )
    .bind(person_id)
    .execute(&mut *tx)
    .await?
    .rows_affected())
}
