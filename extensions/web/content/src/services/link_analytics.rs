//! Campaign and content journey analytics over recorded clicks.

use sqlx::PgPool;
use std::sync::Arc;
use systemprompt::identifiers::{CampaignId, ContentId};

use crate::repository::LinkAnalyticsRepository;
use systemprompt_web_shared::error::BlogError;
use systemprompt_web_shared::models::{CampaignPerformance, ContentJourneyNode};

#[derive(Debug, Clone)]
pub struct LinkAnalyticsService {
    repo: LinkAnalyticsRepository,
}

impl LinkAnalyticsService {
    #[must_use]
    pub const fn new(pool: Arc<PgPool>) -> Self {
        Self {
            repo: LinkAnalyticsRepository::new(pool),
        }
    }

    pub async fn find_campaign_performance(
        &self,
        campaign_id: &str,
    ) -> Result<Option<CampaignPerformance>, BlogError> {
        let campaign_id = CampaignId::new(campaign_id.to_owned());
        self.repo
            .find_campaign_performance(&campaign_id)
            .await
            .map_err(BlogError::from)
    }

    pub async fn list_content_journey(
        &self,
        content_id: &ContentId,
    ) -> Result<Vec<ContentJourneyNode>, BlogError> {
        self.repo
            .list_content_journey(content_id)
            .await
            .map_err(BlogError::from)
    }
}
