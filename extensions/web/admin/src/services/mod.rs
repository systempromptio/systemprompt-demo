//! Service layer between the admin handlers and the repositories.

pub(crate) mod auth;
pub(crate) mod jobs_service;
pub(crate) mod onboarding;
pub(crate) mod secret_service;

pub(crate) use systemprompt_web_governance::device_service;
