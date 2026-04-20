use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};

#[derive(Debug)]
pub enum HashError {
    Hash(argon2::password_hash::Error),
}

impl std::fmt::Display for HashError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HashError::Hash(err) => write!(f, "password hashing failed: {err}"),
        }
    }
}

impl std::error::Error for HashError {}

impl From<argon2::password_hash::Error> for HashError {
    fn from(err: argon2::password_hash::Error) -> Self {
        HashError::Hash(err)
    }
}

pub fn hash_password(password: &str) -> Result<String, HashError> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)?
        .to_string();
    Ok(hash)
}

pub fn verify_password(hash: &str, password: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(verify_password(&hash, "correct horse battery staple"));
        assert!(!verify_password(&hash, "wrong password"));
    }

    #[test]
    fn rejects_malformed_hash() {
        assert!(!verify_password("not-a-real-hash", "anything"));
    }
}
