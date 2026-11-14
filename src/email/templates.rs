use super::Email;

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn build(
    to: &str,
    subject: String,
    greeting: &str,
    lead: &str,
    action: &str,
    link: &str,
    tail: &str,
) -> Email {
    let text = format!("{greeting}\n\n{lead}\n\n{action}: {link}\n\n{tail}\n");
    let html = format!(
        "<p>{}</p><p>{}</p><p><a href=\"{}\">{}</a></p><p>{}</p>",
        escape_html(greeting),
        escape_html(lead),
        escape_html(link),
        escape_html(action),
        escape_html(tail),
    );
    Email {
        to: to.to_owned(),
        subject,
        text,
        html,
    }
}

pub fn verification_email(to: &str, name: &str, link: &str) -> Email {
    build(
        to,
        "Confirm your email for Dearest".to_owned(),
        &format!("Hi {name},"),
        "Confirm your email address to finish your Dearest account.",
        "Confirm your email",
        link,
        "The link works for 24 hours. If you did not create an account, ignore this email.",
    )
}

pub fn reset_email(to: &str, name: &str, link: &str) -> Email {
    build(
        to,
        "Reset your Dearest password".to_owned(),
        &format!("Hi {name},"),
        "Use the link below to choose a new password.",
        "Reset your password",
        link,
        "The link works for 1 hour. If you did not ask for a reset, ignore this email.",
    )
}

pub fn invite_email(to: &str, inviter_name: &str, tenant_name: &str, link: &str) -> Email {
    build(
        to,
        format!("{inviter_name} invited you to {tenant_name} on Dearest"),
        "Hello,",
        &format!("{inviter_name} invited you to join {tenant_name} on Dearest."),
        "Accept the invite",
        link,
        "The link works for 7 days.",
    )
}

/// Return the token from the first link in the text body of an email.
/// Tests use this function.
pub fn extract_token(email: &Email) -> String {
    let start = email.text.find("http").expect("email text has no link");
    let link = email.text[start..]
        .split_whitespace()
        .next()
        .unwrap_or_default();
    link.rsplit(['=', '/'])
        .next()
        .unwrap_or_default()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verification_email_contains_link() {
        let link = "https://app.test/verify-email/confirm?token=abc";
        let email = verification_email("a@example.com", "Alex", link);
        assert_eq!(email.to, "a@example.com");
        assert_eq!(email.subject, "Confirm your email for Dearest");
        assert!(email.text.contains(link));
        assert!(email.html.contains(link));
    }

    #[test]
    fn reset_email_contains_link() {
        let link = "https://app.test/reset-password?token=abc";
        let email = reset_email("a@example.com", "Alex", link);
        assert_eq!(email.subject, "Reset your Dearest password");
        assert!(email.text.contains(link));
        assert!(email.html.contains(link));
    }

    #[test]
    fn invite_email_names_inviter_and_tenant() {
        let link = "https://app.test/invite/abc";
        let email = invite_email("s@example.com", "Alex", "Alex & Sam", link);
        assert_eq!(email.subject, "Alex invited you to Alex & Sam on Dearest");
        assert!(email.text.contains(link));
        assert!(email.html.contains("Alex &amp; Sam"));
        assert!(!email.html.contains("Alex & Sam"));
    }

    #[test]
    fn extract_token_reads_query_and_path_links() {
        let a = verification_email(
            "a@example.com",
            "Alex",
            "https://app.test/verify-email/confirm?token=tok_1-x",
        );
        assert_eq!(extract_token(&a), "tok_1-x");
        let b = invite_email(
            "a@example.com",
            "Alex",
            "W",
            "https://app.test/invite/tok_2-y",
        );
        assert_eq!(extract_token(&b), "tok_2-y");
    }
}
