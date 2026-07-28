//! Who is asking, and therefore how much of the pulse they are owed.
//!
//! The pulse is one endpoint serving three audiences, and the decision about
//! which one a caller belongs to is made here rather than in the browser. That
//! is the whole point: a client that is told its own role will eventually be
//! asked to enforce it, and `GET /admin/auth/me` deliberately omits `is_admin`
//! for exactly that reason. The pane renders whatever shape it is handed and
//! has no way to ask for a richer one.
//!
//! # Why the embed token and not the session cookie
//!
//! Both would work — the token is minted for whoever owned the cookie. The
//! token wins because the pi router is mounted at the site root without
//! `user_context_middleware`, so a cookie path here would mean a second,
//! parallel implementation of "who is this" living next to the first. The token
//! is already verified on this route, already carries a `UserId`, and is
//! already revocable through `share_token_version`. One credential, one answer.
//!
//! # Why a missing token is not an error
//!
//! An anonymous visitor cannot mint an embed token at all — `/embed-token`
//! reads the session cookie. If the pulse demanded one, the tier the homepage
//! most wants to reach would be the only tier that could never see it. So the
//! token is optional and its absence is a fact about the caller rather than a
//! failure: no token means [`Tier::Anonymous`], which is served lifetime totals
//! and nothing else.

use std::sync::Arc;

use sqlx::PgPool;

use systemprompt_web_governance::repositories::user_access;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Tier {
    Anonymous,
    Member,
    Admin,
}

impl Tier {
    pub(super) const COUNT: usize = 3;

    pub(super) const fn index(self) -> usize {
        match self {
            Self::Anonymous => 0,
            Self::Member => 1,
            Self::Admin => 2,
        }
    }
}

pub(super) async fn resolve(pool: &Arc<PgPool>, raw: &str) -> Tier {
    if raw.is_empty() {
        return Tier::Anonymous;
    }
    let Some(user_id) = super::auth::authenticate(pool, raw).await else {
        return Tier::Anonymous;
    };

    match user_access::find_user_access(pool, &user_id).await {
        Ok(Some(access)) if access.roles.iter().any(|r| r == "admin") => Tier::Admin,
        Ok(_) => Tier::Member,
        Err(e) => {
            tracing::warn!(error = %e, user_id = %user_id, "could not read roles for the pulse tier");
            Tier::Member
        },
    }
}
