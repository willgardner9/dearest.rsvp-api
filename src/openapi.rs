//! The OpenAPI document of the API. The served router and the spec come from one function.

use utoipa::{
    Modify, OpenApi,
    openapi::security::{ApiKey, ApiKeyValue, SecurityScheme},
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{AppState, auth, auth::session::COOKIE_NAME, error::ErrorBody, invites, tenants};

#[derive(OpenApi)]
#[openapi(modifiers(&SessionCookie), components(schemas(ErrorBody)))]
struct ApiDoc;

/// Add the session cookie as the `session` security scheme.
struct SessionCookie;

impl Modify for SessionCookie {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "session",
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new(COOKIE_NAME))),
        );
    }
}

/// All routes of the API, with their spec. `app()` adds the state and the layers.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(crate::health))
        .nest("/auth", auth::router())
        .nest(
            "/tenants",
            tenants::router().merge(invites::tenant_routes()),
        )
        .nest("/invites", invites::public_router())
}

/// The OpenAPI document. It needs no state and no database.
pub fn spec() -> utoipa::openapi::OpenApi {
    let mut spec = router().into_openapi();
    // Cargo gives an empty description and an empty license name. Remove them.
    spec.info.description = None;
    spec.info.license = None;
    spec
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    const METHODS: [&str; 9] = [
        "get", "put", "post", "delete", "options", "head", "patch", "trace", "query",
    ];

    #[test]
    fn every_response_has_a_description() {
        let spec = serde_json::to_value(spec()).unwrap();
        for (path, item) in spec["paths"].as_object().unwrap() {
            for method in METHODS {
                let Some(operation) = item.get(method) else {
                    continue;
                };
                for (code, response) in operation["responses"].as_object().unwrap() {
                    let description = response["description"].as_str().unwrap_or("");
                    assert!(
                        !description.is_empty(),
                        "{method} {path} {code} has no description"
                    );
                }
            }
        }
    }

    #[test]
    fn info_has_no_empty_fields() {
        let spec = serde_json::to_value(spec()).unwrap();
        let info = &spec["info"];
        assert!(info.get("license").is_none());
        if let Some(description) = info.get("description") {
            assert!(!description.as_str().unwrap().is_empty());
        }
    }

    #[test]
    fn spec_lists_every_route() {
        let mut found = BTreeSet::new();
        for (path, item) in spec().paths.paths {
            let item = serde_json::to_value(item).unwrap();
            for method in METHODS {
                if item.get(method).is_some() {
                    found.insert((method.to_uppercase(), path.clone()));
                }
            }
        }
        let expected: BTreeSet<(String, String)> = [
            ("GET", "/health"),
            ("POST", "/auth/signup"),
            ("POST", "/auth/login"),
            ("POST", "/auth/logout"),
            ("POST", "/auth/verify-email"),
            ("POST", "/auth/verify-email/resend"),
            ("POST", "/auth/password-reset"),
            ("POST", "/auth/password-reset/confirm"),
            ("GET", "/auth/me"),
            ("POST", "/tenants"),
            ("GET", "/tenants/{tenant_id}"),
            ("GET", "/tenants/{tenant_id}/members"),
            ("GET", "/tenants/{tenant_id}/invites"),
            ("POST", "/tenants/{tenant_id}/invites"),
            ("DELETE", "/tenants/{tenant_id}/invites/{invite_id}"),
            ("GET", "/invites/{token}"),
            ("POST", "/invites/{token}/accept"),
        ]
        .into_iter()
        .map(|(method, path)| (method.to_owned(), path.to_owned()))
        .collect();
        assert_eq!(found, expected);
    }

    #[test]
    fn operation_ids_are_unique_and_named() {
        let mut ids = Vec::new();
        for (_, item) in spec().paths.paths {
            let item = serde_json::to_value(item).unwrap();
            for method in METHODS {
                if let Some(id) = item.get(method).and_then(|op| op.get("operationId")) {
                    ids.push(id.as_str().unwrap().to_owned());
                }
            }
        }
        ids.sort();
        let mut expected = vec![
            "getHealth",
            "signup",
            "login",
            "logout",
            "getMe",
            "verifyEmail",
            "resendVerificationEmail",
            "requestPasswordReset",
            "confirmPasswordReset",
            "createTenant",
            "getTenant",
            "listMembers",
            "createInvite",
            "listInvites",
            "cancelInvite",
            "getInvitePreview",
            "acceptInvite",
        ];
        expected.sort();
        assert_eq!(ids, expected);
    }

    #[test]
    fn accept_body_is_optional() {
        let spec = serde_json::to_value(spec()).unwrap();
        let body = &spec["paths"]["/invites/{token}/accept"]["post"]["requestBody"];
        assert_eq!(
            body["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/NewAccountBody"
        );
        assert!(body.get("required").is_none());
    }

    #[test]
    fn session_scheme_is_the_cookie() {
        let spec = serde_json::to_value(spec()).unwrap();
        assert_eq!(
            spec["components"]["securitySchemes"]["session"],
            serde_json::json!({"type": "apiKey", "in": "cookie", "name": COOKIE_NAME})
        );
    }

    #[test]
    fn health_lists_500() {
        let spec = serde_json::to_value(spec()).unwrap();
        let responses = spec["paths"]["/health"]["get"]["responses"]
            .as_object()
            .unwrap();
        let codes: BTreeSet<&str> = responses.keys().map(String::as_str).collect();
        assert_eq!(codes, BTreeSet::from(["200", "500", "503"]));
    }

    #[test]
    fn invite_inviter_name_is_required_and_nullable() {
        let spec = serde_json::to_value(spec()).unwrap();
        let invite = &spec["components"]["schemas"]["Invite"];
        let required = invite["required"].as_array().unwrap();
        assert!(required.iter().any(|name| name == "invited_by_name"));
        assert_eq!(
            invite["properties"]["invited_by_name"]["type"],
            serde_json::json!(["string", "null"])
        );
    }
}
