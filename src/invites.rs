use std::collections::HashMap;

use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{Duration, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set, SqlErr, TransactionTrait,
    sea_query::{Expr, OnConflict},
};
use serde::Deserialize;
use utoipa::{ToSchema, openapi::RefOr};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    AppState,
    auth::{
        extract::{MaybeUser, TenantMember},
        mark_verified,
        password::hash_password,
        session::{create_session, session_cookie},
        token::{hash_token, new_token},
        validate::{normalize_email, validate_name, validate_password},
    },
    dto::{AcceptedInvite, CreatedInvite, Invite, InvitePreview},
    email::{spawn_send, templates},
    entities::{invites, tenants, user_tenant_memberships, users},
    error::{AppError, AppResult, ErrorBody},
    json::ApiJson,
    rate_limit::check,
};

const INVITE_DAYS: i64 = 7;
const UPSERT_ATTEMPTS: usize = 3;

/// Routes under `/tenants`: the invites of one tenant. A member calls them.
pub fn tenant_routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(create_invite, list_invites))
        .routes(routes!(cancel_invite))
}

/// Routes under `/invites`: a person with the token calls them.
pub fn public_router() -> OpenApiRouter<AppState> {
    let mut router = OpenApiRouter::new()
        .routes(routes!(preview))
        .routes(routes!(accept));
    // A logged-in user sends no body to `accept`. utoipa has no `required = false`
    // for a request body, so clear the flag here.
    if let Some(RefOr::T(body)) = router
        .get_openapi_mut()
        .paths
        .paths
        .get_mut("/{token}/accept")
        .and_then(|item| item.post.as_mut())
        .and_then(|operation| operation.request_body.as_mut())
    {
        body.required = None;
    }
    router
}

#[derive(Deserialize, ToSchema)]
struct InviteBody {
    email: String,
}

#[utoipa::path(post, path = "/{tenant_id}/invites", operation_id = "createInvite", tag = "invites", params(("tenant_id" = Uuid, Path)),
    request_body = InviteBody, security(("session" = [])), responses(
    (status = 201, body = CreatedInvite),
    (status = 400, description = "invalid_input", body = ErrorBody),
    (status = 401, description = "unauthenticated", body = ErrorBody),
    (status = 403, description = "forbidden_origin, email_not_verified", body = ErrorBody),
    (status = 404, description = "not_found", body = ErrorBody),
    (status = 409, description = "already_member", body = ErrorBody),
    (status = 429, description = "rate_limited", body = ErrorBody),
    (status = 500, description = "internal", body = ErrorBody),
))]
async fn create_invite(
    State(state): State<AppState>,
    member: TenantMember,
    ApiJson(body): ApiJson<InviteBody>,
) -> AppResult<(StatusCode, Json<CreatedInvite>)> {
    check(
        &state.limits.invite_user,
        &member.session.user.id.to_string(),
    )?;
    let email = normalize_email(&body.email)?;
    let tenant = &member.tenant;

    let existing_member = user_tenant_memberships::Entity::find()
        .inner_join(users::Entity)
        .filter(users::Column::Email.eq(email.as_str()))
        .filter(user_tenant_memberships::Column::TenantId.eq(tenant.id))
        .one(&state.db)
        .await?
        .is_some();
    if existing_member {
        return Err(AppError::AlreadyMember);
    }

    // Two requests at the same time can both try to insert. The partial unique
    // index stops one of them, and that one then takes the replace path.
    let mut upserted = None;
    for _ in 0..UPSERT_ATTEMPTS {
        upserted = upsert_invite(&state, tenant.id, member.session.user.id, &email).await?;
        if upserted.is_some() {
            break;
        }
    }
    let (invite, token) =
        upserted.ok_or_else(|| AppError::Internal(anyhow::anyhow!("invite insert conflict")))?;

    let link = format!("{}/invite/{token}", state.config.app_base_url);
    spawn_send(
        &state,
        templates::invite_email(&email, &member.session.user.name, &tenant.name, &link),
    );
    Ok((
        StatusCode::CREATED,
        Json(CreatedInvite {
            id: invite.id,
            email: invite.email,
            expires_at: invite.expires_at,
        }),
    ))
}

/// Replace the token of the open invite for this email, or insert a new invite.
/// Return `None` if a request at the same time inserted the open invite first.
async fn upsert_invite(
    state: &AppState,
    tenant_id: Uuid,
    inviter_id: Uuid,
    email: &str,
) -> AppResult<Option<(invites::Model, String)>> {
    let now = Utc::now();
    let expires_at = (now + Duration::days(INVITE_DAYS)).fixed_offset();
    let (token, hash) = new_token();
    let txn = state.db.begin().await?;
    let open = invites::Entity::find()
        .filter(invites::Column::TenantId.eq(tenant_id))
        .filter(invites::Column::Email.eq(email))
        .filter(invites::Column::AcceptedAt.is_null())
        .filter(invites::Column::CancelledAt.is_null())
        .lock_exclusive()
        .one(&txn)
        .await?;
    let invite = match open {
        Some(open) => {
            let mut active: invites::ActiveModel = open.into();
            active.token_hash = Set(hash);
            active.expires_at = Set(expires_at);
            active.invited_by = Set(inviter_id);
            active.update(&txn).await?
        }
        None => {
            let inserted = invites::ActiveModel {
                id: Set(Uuid::now_v7()),
                tenant_id: Set(tenant_id),
                email: Set(email.to_owned()),
                invited_by: Set(inviter_id),
                token_hash: Set(hash),
                expires_at: Set(expires_at),
                accepted_at: Set(None),
                cancelled_at: Set(None),
                created_at: Set(now.fixed_offset()),
            }
            .insert(&txn)
            .await;
            match inserted {
                Ok(invite) => invite,
                Err(err) if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {
                    return Ok(None);
                }
                Err(err) => return Err(err.into()),
            }
        }
    };
    txn.commit().await?;
    Ok(Some((invite, token)))
}

#[utoipa::path(get, path = "/{tenant_id}/invites", operation_id = "listInvites", tag = "invites", params(("tenant_id" = Uuid, Path)),
    security(("session" = [])), responses(
    (status = 200, description = "The open invites.", body = Vec<Invite>),
    (status = 401, description = "unauthenticated", body = ErrorBody),
    (status = 403, description = "email_not_verified", body = ErrorBody),
    (status = 404, description = "not_found", body = ErrorBody),
    (status = 500, description = "internal", body = ErrorBody),
))]
async fn list_invites(
    State(state): State<AppState>,
    member: TenantMember,
) -> AppResult<Json<Vec<Invite>>> {
    let rows = invites::Entity::find()
        .filter(invites::Column::TenantId.eq(member.tenant.id))
        .filter(invites::Column::AcceptedAt.is_null())
        .filter(invites::Column::CancelledAt.is_null())
        .filter(invites::Column::ExpiresAt.gt(Utc::now()))
        .order_by_asc(invites::Column::CreatedAt)
        .find_also_related(users::Entity)
        .all(&state.db)
        .await?;
    let list: Vec<Invite> = rows
        .into_iter()
        .map(|(invite, inviter)| Invite {
            id: invite.id,
            email: invite.email,
            invited_by_name: inviter.map(|user| user.name),
            expires_at: invite.expires_at,
        })
        .collect();
    Ok(Json(list))
}

// 404 also covers an invite ID that is not valid, or an invite that is not open.
#[utoipa::path(delete, path = "/{tenant_id}/invites/{invite_id}", operation_id = "cancelInvite", tag = "invites",
    params(("tenant_id" = Uuid, Path), ("invite_id" = Uuid, Path)),
    security(("session" = [])), responses(
    (status = 204, description = "Invite cancelled."),
    (status = 401, description = "unauthenticated", body = ErrorBody),
    (status = 403, description = "forbidden_origin, email_not_verified", body = ErrorBody),
    (status = 404, description = "not_found", body = ErrorBody),
    (status = 500, description = "internal", body = ErrorBody),
))]
async fn cancel_invite(
    State(state): State<AppState>,
    member: TenantMember,
    Path(params): Path<HashMap<String, String>>,
) -> AppResult<StatusCode> {
    let invite_id: Uuid = params
        .get("invite_id")
        .and_then(|value| value.parse().ok())
        .ok_or(AppError::NotFound)?;
    let now = Utc::now().fixed_offset();
    let cancelled = invites::Entity::update_many()
        .col_expr(invites::Column::CancelledAt, Expr::value(now))
        .filter(invites::Column::Id.eq(invite_id))
        .filter(invites::Column::TenantId.eq(member.tenant.id))
        .filter(invites::Column::AcceptedAt.is_null())
        .filter(invites::Column::CancelledAt.is_null())
        .filter(invites::Column::ExpiresAt.gt(now))
        .exec(&state.db)
        .await?;
    if cancelled.rows_affected == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Find an invite that is open (not accepted, not cancelled) and not expired.
async fn find_open_invite(
    db: &impl ConnectionTrait,
    token: &str,
    lock: bool,
) -> AppResult<invites::Model> {
    let mut query = invites::Entity::find()
        .filter(invites::Column::TokenHash.eq(hash_token(token)))
        .filter(invites::Column::AcceptedAt.is_null())
        .filter(invites::Column::CancelledAt.is_null())
        .filter(invites::Column::ExpiresAt.gt(Utc::now()));
    if lock {
        query = query.lock_exclusive();
    }
    query.one(db).await?.ok_or(AppError::NotFound)
}

async fn account_exists(db: &impl ConnectionTrait, email: &str) -> Result<bool, sea_orm::DbErr> {
    Ok(users::Entity::find()
        .filter(users::Column::Email.eq(email))
        .one(db)
        .await?
        .is_some())
}

#[utoipa::path(get, path = "/{token}", operation_id = "getInvitePreview", tag = "invites", params(("token" = String, Path)), responses(
    (status = 200, body = InvitePreview),
    (status = 404, description = "not_found", body = ErrorBody),
    (status = 500, description = "internal", body = ErrorBody),
))]
async fn preview(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> AppResult<Json<InvitePreview>> {
    let invite = find_open_invite(&state.db, &token, false).await?;
    let (tenant, inviter, account_exists) = tokio::try_join!(
        tenants::Entity::find_by_id(invite.tenant_id).one(&state.db),
        users::Entity::find_by_id(invite.invited_by).one(&state.db),
        account_exists(&state.db, &invite.email),
    )?;
    let tenant = tenant.ok_or(AppError::NotFound)?;
    let inviter = inviter.ok_or(AppError::NotFound)?;
    Ok(Json(InvitePreview {
        tenant_id: tenant.id,
        tenant_name: tenant.name,
        inviter_name: inviter.name,
        email: invite.email,
        account_exists,
    }))
}

#[derive(Deserialize, ToSchema)]
struct NewAccountBody {
    name: String,
    password: String,
}

/// Accept an invite. A logged-in user sends no body. A new user sends `NewAccountBody`.
// Logged in: 403 invite_email_mismatch, 409 already_member.
// Logged out: 401 unauthenticated if the account exists, 400 invalid_input for a bad body.
#[utoipa::path(post, path = "/{token}/accept", operation_id = "acceptInvite", tag = "invites", params(("token" = String, Path)),
    request_body = NewAccountBody,
    security((), ("session" = [])), responses(
    (status = 200, description = "Joined. A new user also gets the session cookie.", body = AcceptedInvite),
    (status = 400, description = "invalid_input", body = ErrorBody),
    (status = 401, description = "unauthenticated", body = ErrorBody),
    (status = 403, description = "forbidden_origin, invite_email_mismatch", body = ErrorBody),
    (status = 404, description = "not_found", body = ErrorBody),
    (status = 409, description = "already_member", body = ErrorBody),
    (status = 500, description = "internal", body = ErrorBody),
))]
async fn accept(
    State(state): State<AppState>,
    MaybeUser(session): MaybeUser,
    Path(token): Path<String>,
    body: Bytes,
) -> AppResult<(CookieJar, Json<AcceptedInvite>)> {
    let now = Utc::now().fixed_offset();
    match session {
        Some(session) => {
            let txn = state.db.begin().await?;
            let invite = find_open_invite(&txn, &token, true).await?;
            if session.user.email != invite.email {
                return Err(AppError::InviteEmailMismatch);
            }
            let joined = add_membership(&txn, session.user.id, invite.tenant_id).await?;
            if joined {
                mark_verified(&txn, session.user.id).await?;
            }
            close_invite(&txn, &invite).await?;
            txn.commit().await?;
            if !joined {
                return Err(AppError::AlreadyMember);
            }
            Ok((
                CookieJar::new(),
                Json(AcceptedInvite {
                    tenant_id: invite.tenant_id,
                }),
            ))
        }
        None => {
            // Cheap checks first, so a bad token does not cost a password hash.
            let invite = find_open_invite(&state.db, &token, false).await?;
            if account_exists(&state.db, &invite.email).await? {
                return Err(AppError::Unauthenticated);
            }
            let new_account: NewAccountBody =
                serde_json::from_slice(&body).map_err(|_| AppError::InvalidInput)?;
            let name = validate_name(&new_account.name)?;
            validate_password(&new_account.password)?;
            let password_hash = hash_password(new_account.password).await?;

            let txn = state.db.begin().await?;
            let invite = find_open_invite(&txn, &token, true).await?;
            let inserted = users::ActiveModel {
                id: Set(Uuid::now_v7()),
                email: Set(invite.email.clone()),
                name: Set(name),
                password_hash: Set(password_hash),
                email_verified_at: Set(Some(now)),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&txn)
            .await;
            let user = match inserted {
                Ok(user) => user,
                // An account for this email appeared in the meantime.
                Err(err) if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {
                    return Err(AppError::Unauthenticated);
                }
                Err(err) => return Err(err.into()),
            };
            add_membership(&txn, user.id, invite.tenant_id).await?;
            close_invite(&txn, &invite).await?;
            let session_token = create_session(&txn, user.id).await?;
            txn.commit().await?;
            let jar =
                CookieJar::new().add(session_cookie(&session_token, state.config.cookie_secure));
            Ok((
                jar,
                Json(AcceptedInvite {
                    tenant_id: invite.tenant_id,
                }),
            ))
        }
    }
}

/// Insert a membership. Return false if the user is already a member.
async fn add_membership(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    tenant_id: Uuid,
) -> AppResult<bool> {
    let inserted = user_tenant_memberships::Entity::insert(user_tenant_memberships::ActiveModel {
        user_id: Set(user_id),
        tenant_id: Set(tenant_id),
        created_at: Set(Utc::now().fixed_offset()),
    })
    .on_conflict(
        OnConflict::columns([
            user_tenant_memberships::Column::UserId,
            user_tenant_memberships::Column::TenantId,
        ])
        .do_nothing()
        .to_owned(),
    )
    .exec_without_returning(db)
    .await?;
    Ok(inserted > 0)
}

async fn close_invite(db: &impl ConnectionTrait, invite: &invites::Model) -> AppResult<()> {
    invites::Entity::update_many()
        .col_expr(
            invites::Column::AcceptedAt,
            Expr::value(Utc::now().fixed_offset()),
        )
        .filter(invites::Column::Id.eq(invite.id))
        .exec(db)
        .await?;
    Ok(())
}
