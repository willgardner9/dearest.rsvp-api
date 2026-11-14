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
    router().into_openapi()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    const METHODS: [&str; 9] = [
        "get", "put", "post", "delete", "options", "head", "patch", "trace", "query",
    ];

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
}
