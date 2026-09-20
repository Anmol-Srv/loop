use sqlx::PgConnection;
use uuid::Uuid;

use crate::errors::AppResult;
use crate::models::token::hash_token;

/// No O/0 or I/1 — codes get read aloud and typed by hand.
const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// Codes are valid for 48 hours.
pub const VALID_HOURS: i64 = 48;

/// Three groups of four, e.g. `K7QF-M2XT-9PDR`. 60 bits of entropy from
/// uuid's CSPRNG — the same source the raw tokens use, so no extra dependency.
/// The alphabet divides 256 exactly, so the modulo is unbiased.
pub fn generate() -> String {
    let bytes: Vec<u8> = uuid::Uuid::new_v4()
        .as_bytes()
        .iter()
        .take(12)
        .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()])
        .collect();

    bytes
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join("-")
}

/// Issue a code, voiding whatever live code the person had. Both halves run on
/// the caller's connection so they share one transaction — the partial unique
/// index `setup_code_one_live_per_person` rejects the insert otherwise.
pub async fn issue(tx: &mut PgConnection, person_id: Uuid) -> AppResult<String> {
    sqlx::query("UPDATE setup_code SET used_at = now() WHERE person_id = $1 AND used_at IS NULL")
        .bind(person_id)
        .execute(&mut *tx)
        .await?;

    let raw = generate();
    sqlx::query(
        "INSERT INTO setup_code (person_id, code_hash, expires_at)
         VALUES ($1, $2, now() + ($3 || ' hours')::interval)",
    )
    .bind(person_id)
    .bind(hash_token(&raw))
    .bind(VALID_HOURS.to_string())
    .execute(&mut *tx)
    .await?;

    Ok(raw)
}

/// Spend a code. Unknown, already used and expired codes all return `None`.
/// Single statement, so two concurrent redemptions cannot both win.
pub async fn redeem(tx: &mut PgConnection, raw: &str) -> AppResult<Option<Uuid>> {
    let person_id = sqlx::query_scalar::<_, Uuid>(
        "UPDATE setup_code SET used_at = now()
         WHERE code_hash = $1 AND used_at IS NULL AND expires_at > now()
         RETURNING person_id",
    )
    .bind(hash_token(&raw.trim().to_uppercase()))
    .fetch_optional(&mut *tx)
    .await?;

    Ok(person_id)
}
