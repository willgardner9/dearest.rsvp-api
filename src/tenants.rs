use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set, TransactionTrait,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{
        extract::{TenantMember, VerifiedUser},
        validate::validate_text,
    },
    dto::{Member, Tenant},
    entities::{tenants, user_tenant_memberships, users},
    error::AppResult,
    json::ApiJson,
};

const MAX_TENANT_NAME_LEN: usize = 200;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", post(create_tenant))
        .route("/{tenant_id}", get(get_tenant))
        .route("/{tenant_id}/members", get(list_members))
}

#[derive(Deserialize)]
struct CreateTenantBody {
    name: String,
}

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

async fn get_tenant(member: TenantMember) -> Json<Tenant> {
    Json(Tenant {
        id: member.tenant.id,
        name: member.tenant.name,
    })
}

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
