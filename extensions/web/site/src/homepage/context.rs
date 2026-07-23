//! Template context assembled for `homepage.html`.

use serde::Serialize;

use super::config::HomepageConfig;

#[derive(Debug, Serialize)]
pub(super) struct HomepageContext<'a> {
    site: HomepageSite<'a>,
}

#[derive(Debug, Serialize)]
struct HomepageSite<'a> {
    homepage: &'a HomepageConfig,
}

impl<'a> HomepageContext<'a> {
    pub(super) const fn new(homepage: &'a HomepageConfig) -> Self {
        Self {
            site: HomepageSite { homepage },
        }
    }
}
