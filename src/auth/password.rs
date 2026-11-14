use std::sync::LazyLock;

use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{Error as HashError, PasswordHasher, PasswordVerifier, phc::PasswordHash},
};

use crate::error::{AppError, AppResult};

/// argon2id with the OWASP parameters: 19 MiB memory, 2 iterations, 1 lane.
fn argon2() -> Argon2<'static> {
    let params = Params::new(19456, 2, 1, None).expect("argon2 parameters are valid");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

fn hash_blocking(password: &str) -> AppResult<String> {
    argon2()
        .hash_password(password.as_bytes())
        .map(|hash| hash.to_string())
        .map_err(|err| AppError::Internal(anyhow::anyhow!("password hash failed: {err}")))
}

fn verify_blocking(password: &str, hash: &str) -> AppResult<bool> {
    let parsed = PasswordHash::new(hash)
        .map_err(|err| AppError::Internal(anyhow::anyhow!("stored hash is invalid: {err}")))?;
    match argon2().verify_password(password.as_bytes(), &parsed) {
        Ok(()) => Ok(true),
        Err(HashError::PasswordInvalid) => Ok(false),
        Err(err) => Err(AppError::Internal(anyhow::anyhow!(
            "password check failed: {err}"
        ))),
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|err| AppError::Internal(err.into()))?
}

pub async fn hash_password(password: String) -> AppResult<String> {
    blocking(move || hash_blocking(&password)).await
}

pub async fn verify_password(password: String, hash: String) -> AppResult<bool> {
    blocking(move || verify_blocking(&password, &hash)).await
}

/// A fixed hash, made one time. Login checks against it for an unknown email.
static DUMMY_HASH: LazyLock<String> =
    LazyLock::new(|| hash_blocking("dearest-dummy-password").expect("dummy hash"));

/// Do the same work as a real check, so response time does not show if an account exists.
pub async fn verify_dummy(password: String) -> AppResult<()> {
    blocking(move || verify_blocking(&password, &DUMMY_HASH).map(|_| ())).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hash_then_verify() {
        let hash = hash_password("correct horse".to_owned()).await.unwrap();
        assert!(
            verify_password("correct horse".to_owned(), hash.clone())
                .await
                .unwrap()
        );
        assert!(!verify_password("other".to_owned(), hash).await.unwrap());
    }

    #[tokio::test]
    async fn hash_uses_argon2id_params() {
        let hash = hash_password("x".to_owned()).await.unwrap();
        assert!(
            hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
            "{hash}"
        );
    }

    #[tokio::test]
    async fn verify_dummy_returns_ok() {
        verify_dummy("anything".to_owned()).await.unwrap();
    }
}
