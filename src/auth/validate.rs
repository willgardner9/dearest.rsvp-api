use crate::error::{AppError, AppResult};

const MAX_EMAIL_LEN: usize = 254;
const MAX_NAME_LEN: usize = 100;

/// Trim and lowercase an email. Every email input goes through this function.
pub fn normalize_email(input: &str) -> AppResult<String> {
    let email = input.trim().to_lowercase();
    let mut parts = email.split('@');
    let valid = email.len() <= MAX_EMAIL_LEN
        && matches!(
            (parts.next(), parts.next(), parts.next()),
            (Some(local), Some(domain), None) if !local.is_empty() && !domain.is_empty()
        );
    if valid {
        Ok(email)
    } else {
        Err(AppError::InvalidInput)
    }
}

/// Trim a text. It must have 1 to `max` characters.
pub fn validate_text(input: &str, max: usize) -> AppResult<String> {
    let text = input.trim();
    if text.is_empty() || text.chars().count() > max {
        Err(AppError::InvalidInput)
    } else {
        Ok(text.to_owned())
    }
}

pub fn validate_name(input: &str) -> AppResult<String> {
    validate_text(input, MAX_NAME_LEN)
}

/// The only password rule: not empty.
pub fn validate_password(input: &str) -> AppResult<()> {
    if input.is_empty() {
        Err(AppError::InvalidInput)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;

    #[test]
    fn normalize_email_trims_and_lowercases() {
        assert_eq!(
            normalize_email(" Alex@Example.COM ").unwrap(),
            "alex@example.com"
        );
    }

    #[test]
    fn normalize_email_rejects_bad_input() {
        for bad in ["", "alex", "@x", "a@", "a@b@c", "   "] {
            assert!(
                matches!(normalize_email(bad), Err(AppError::InvalidInput)),
                "{bad:?}"
            );
        }
        let long = format!("{}@example.com", "a".repeat(250));
        assert!(matches!(
            normalize_email(&long),
            Err(AppError::InvalidInput)
        ));
    }

    #[test]
    fn password_rule_rejects_only_empty() {
        assert!(matches!(validate_password(""), Err(AppError::InvalidInput)));
        assert!(validate_password("a").is_ok());
    }

    #[test]
    fn name_rules() {
        assert!(matches!(validate_name("  "), Err(AppError::InvalidInput)));
        assert_eq!(validate_name("  Alex ").unwrap(), "Alex");
        assert!(matches!(
            validate_name(&"a".repeat(101)),
            Err(AppError::InvalidInput)
        ));
        assert!(validate_text(&"a".repeat(200), 200).is_ok());
        assert!(matches!(
            validate_text(&"a".repeat(201), 200),
            Err(AppError::InvalidInput)
        ));
    }
}
