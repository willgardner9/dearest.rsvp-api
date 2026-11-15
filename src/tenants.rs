use axum::{Json, extract::State, http::StatusCode};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set, TransactionTrait,
};
use serde::Deserialize;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    AppState,
    auth::{
        extract::{TenantMember, VerifiedUser},
        validate::validate_text,
    },
    dto::{Member, Tenant},
    entities::{tenants, user_tenant_memberships, users},
    error::{AppResult, ErrorBody},
    json::ApiJson,
};

const MAX_TENANT_NAME_LEN: usize = 200;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(create_tenant))
        .routes(routes!(get_tenant))
        .routes(routes!(list_members))
}

#[derive(Deserialize, ToSchema)]
struct CreateTenantBody {
    name: String,
}

#[utoipa::path(post, path = "/", operation_id = "createTenant", tag = "tenants", request_body = CreateTenantBody,
    security(("session" = [])), responses(
    (status = 201, description = "Tenant made.", body = Tenant),
    (status = 400, description = "invalid_input", body = ErrorBody),
    (status = 401, description = "unauthenticated", body = ErrorBody),
    (status = 403, description = "forbidden_origin, email_not_verified", body = ErrorBody),
    (status = 500, description = "internal", body = ErrorBody),
))]
async fn create_tenant(
    State(state): State<AppState>,
    VerifiedUser(session): VerifiedUser,
    ApiJson(body): ApiJson<CreateTenantBody>,
) -> AppResult<(StatusCode, Json<Tenant>)> {
    let name = validate_text(&body.name, MAX_TENANT_NAME_LEN)?;
    let now = Utc::now().fixed_offset();
    let txn = state.db.begin().await?;
    let tenant = tenants::ActiveModel {
        id: Set(Uuid::now_v7()),
        name: Set(name),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&txn)
    .await?;
    user_tenant_memberships::ActiveModel {
        user_id: Set(session.user.id),
        tenant_id: Set(tenant.id),
        created_at: Set(now),
    }
    .insert(&txn)
    .await?;
    txn.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(Tenant {
            id: tenant.id,
            name: tenant.name,
        }),
    ))
}

// `TenantMember` gives 404 for a bad ID, a missing tenant, and a tenant of someone else.
#[utoipa::path(get, path = "/{tenant_id}", operation_id = "getTenant", tag = "tenants", params(("tenant_id" = Uuid, Path)),
    security(("session" = [])), responses(
    (status = 200, description = "The tenant.", body = Tenant),
    (status = 401, description = "unauthenticated", body = ErrorBody),
    (status = 403, description = "email_not_verified", body = ErrorBody),
    (status = 404, description = "not_found", body = ErrorBody),
    (status = 500, description = "internal", body = ErrorBody),
))]
async fn get_tenant(member: TenantMember) -> Json<Tenant> {
    Json(Tenant {
        id: member.tenant.id,
        name: member.tenant.name,
    })
}

#[utoipa::path(get, path = "/{tenant_id}/members", operation_id = "listMembers", tag = "tenants", params(("tenant_id" = Uuid, Path)),
    security(("session" = [])), responses(
    (status = 200, description = "The members of the tenant.", body = Vec<Member>),
    (status = 401, description = "unauthenticated", body = ErrorBody),
    (status = 403, description = "email_not_verified", body = ErrorBody),
    (status = 404, description = "not_found", body = ErrorBody),
    (status = 500, description = "internal", body = ErrorBody),
))]
async fn list_members(
    State(state): State<AppState>,
    member: TenantMember,
) -> AppResult<Json<Vec<Member>>> {
    let rows = user_tenant_memberships::Entity::find()
        .filter(user_tenant_memberships::Column::TenantId.eq(member.tenant.id))
        .order_by_asc(user_tenant_memberships::Column::CreatedAt)
        .find_also_related(users::Entity)
        .all(&state.db)
        .await?;
    let members: Vec<Member> = rows
        .into_iter()
        .filter_map(|(_, user)| user)
        .map(|user| Member {
            user_id: user.id,
            name: user.name,
            email: user.email,
        })
        .collect();
    Ok(Json(members))
}
