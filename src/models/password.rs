use std::sync::OnceLock;

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};

use crate::errors::{AppError, AppResult};

/// Minimum password length. No composition rules — see the auth design, §3.
pub const MIN_LENGTH: usize = 12;

/// argon2id with the crate's recommended defaults; the salt comes from the
/// system RNG and travels inside the returned PHC string.
pub fn hash(plain: &str) -> AppResult<String> {
    Argon2::default()
        .hash_password(plain.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(format!("password hashing failed: {e}")))
}

/// False for a wrong password and for a hash we cannot parse — a corrupt hash
/// is not a reason to let someone in.
pub fn verify(plain: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(plain.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// Burn the same work as a real verify when no person matched, so login
/// response timing does not disclose who has an account (§3). Always false.
pub fn verify_dummy(plain: &str) -> bool {
    // ponytail: computed once per process rather than pasted as a literal, so
    // it always carries the current default params.
    static DUMMY: OnceLock<String> = OnceLock::new();
    let dummy = DUMMY.get_or_init(|| {
        hash("timing-equalisation-placeholder").expect("argon2 must hash")
    });
    verify(plain, dummy);
    false
}

/// Twelve characters, and not the email address. Nothing else.
pub fn validate(plain: &str, email: &str) -> AppResult<()> {
    if plain.chars().count() < MIN_LENGTH {
        return Err(AppError::BadRequest(format!(
            "password must be at least {MIN_LENGTH} characters"
        )));
    }
    if plain.eq_ignore_ascii_case(email) {
        return Err(AppError::BadRequest(
            "password must not be your email address".into(),
        ));
    }
    Ok(())
}
