//! Helpers shared across admin handlers.

use std::path::PathBuf;

use serde::Serialize;
use systemprompt::config::ProfileBootstrap;

use crate::error::AdminResult;

#[derive(Debug, Serialize)]
pub(crate) struct ErrorBody {
    pub error: String,
}

pub(crate) fn get_services_path() -> AdminResult<PathBuf> {
    Ok(PathBuf::from(&ProfileBootstrap::get()?.paths.services))
}

pub(crate) fn get_profile_path() -> AdminResult<PathBuf> {
    Ok(PathBuf::from(ProfileBootstrap::get_path()?))
}
