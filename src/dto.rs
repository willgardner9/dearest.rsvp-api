//! The JSON bodies that handlers return.

use chrono::{DateTime, FixedOffset};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

/// The current user and the tenants of the user.
#[derive(Serialize, ToSchema)]
pub struct Me {
    pub user: MeUser,
    pub memberships: Vec<MembershipSummary>,
}

#[derive(Serialize, ToSchema)]
pub struct MeUser {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub email_verified: bool,
}

#[derive(Serialize, ToSchema)]
pub struct MembershipSummary {
    pub tenant_id: Uuid,
    pub tenant_name: String,
}

#[derive(Serialize, ToSchema)]
pub struct Tenant {
    pub id: Uuid,
    pub name: String,
}

#[derive(Serialize, ToSchema)]
pub struct Member {
    pub user_id: Uuid,
    pub name: String,
    pub email: String,
}

#[derive(Serialize, ToSchema)]
pub struct CreatedInvite {
    pub id: Uuid,
    pub email: String,
    pub expires_at: DateTime<FixedOffset>,
}

#[derive(Serialize, ToSchema)]
pub struct Invite {
    pub id: Uuid,
    pub email: String,
    pub invited_by_name: Option<String>,
    pub expires_at: DateTime<FixedOffset>,
}

#[derive(Serialize, ToSchema)]
pub struct InvitePreview {
    pub tenant_id: Uuid,
    pub tenant_name: String,
    pub inviter_name: String,
    pub email: String,
    pub account_exists: bool,
}

#[derive(Serialize, ToSchema)]
pub struct AcceptedInvite {
    pub tenant_id: Uuid,
}

#[derive(Serialize, ToSchema)]
pub struct Health {
    pub status: String,
    pub db: String,
}
