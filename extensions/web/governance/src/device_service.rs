//! Device enrolment and personal access token lifecycle.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use systemprompt::identifiers::UserId;

use crate::error::{GovernanceError, GovernanceResult};
use crate::repositories::bridge::{self, EnrollDeviceParams, EnrolledDevice, IssuedApiKey};

#[derive(Debug)]
pub struct EnrollDeviceInput<'a> {
    pub name: &'a str,
    pub platform: &'a str,
    pub hostname: &'a str,
    pub expires_at: Option<DateTime<Utc>>,
}

pub async fn enroll_device(
    pool: &PgPool,
    user_id: &UserId,
    req: EnrollDeviceInput<'_>,
) -> GovernanceResult<EnrolledDevice> {
    let enrolled = bridge::enroll_device(
        pool,
        user_id,
        EnrollDeviceParams {
            name: req.name,
            platform: req.platform,
            hostname: req.hostname,
            expires_at: req.expires_at,
        },
    )
    .await?;
    Ok(enrolled)
}

pub async fn issue_pat(
    pool: &PgPool,
    user_id: &UserId,
    name: &str,
    expires_at: Option<DateTime<Utc>>,
) -> GovernanceResult<IssuedApiKey> {
    let issued = bridge::issue_api_key(pool, user_id, name, expires_at).await?;
    Ok(issued)
}

pub async fn revoke_pat(pool: &PgPool, user_id: &UserId, id: &str) -> GovernanceResult<()> {
    let revoked = bridge::revoke_api_key(pool, user_id, id).await?;
    if !revoked {
        return Err(GovernanceError::NotFound("PAT not found".to_owned()));
    }
    Ok(())
}

pub async fn revoke_expired_pats_by_name_prefix(
    pool: &PgPool,
    name_prefix: &str,
) -> GovernanceResult<u64> {
    let revoked = bridge::revoke_expired_api_keys_by_name_prefix(pool, name_prefix).await?;
    Ok(revoked)
}

pub async fn revoke_device_cert(pool: &PgPool, user_id: &UserId, id: &str) -> GovernanceResult<()> {
    let revoked = bridge::revoke_device_cert(pool, user_id, id).await?;
    if !revoked {
        return Err(GovernanceError::NotFound("cert not found".to_owned()));
    }
    Ok(())
}
