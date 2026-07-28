//! `Extension` trait implementation for `WebExtension`.
//!
//! The wiring seam: every provider, prerenderer, renderer, schema, migration
//! and job the five web sibling crates expose is advertised to the host runtime
//! here.

use std::sync::Arc;

use axum::Router;

use systemprompt::analytics::AnalyticsService;
use systemprompt::database::Database;
use systemprompt::extension::prelude::*;
use systemprompt::oauth::SessionCreationService;
use systemprompt::traits::{AnalyticsProvider, Job};
use systemprompt::users::UserService;

use crate::assets::web_assets;
use crate::blog::{BlogListPageDataProvider, BlogPostPageDataProvider};
use crate::docs::{DocsContentDataProvider, DocsPageDataProvider};
use crate::extenders::OrgUrlExtender;
use crate::homepage::{HomepagePageDataProvider, HomepagePrerenderer};
use crate::jobs::{
    BundleAdminCssJob, ContentAnalyticsAggregationJob, ContentIngestionJob, ContentPrerenderJob,
    CopyExtensionAssetsJob, GovernanceBootstrapJob, LlmsTxtGenerationJob, PublishPipelineJob,
    RobotsTxtGenerationJob, SitemapGenerationJob,
};
use crate::navigation::NavigationPageDataProvider;
use crate::partials::{
    AgenticMeshAnimationPartialRenderer, ArchitectureDiagramPartialRenderer,
    CliRemoteAnimationPartialRenderer, FooterPartialRenderer, HeadAssetsPartialRenderer,
    HeaderPartialRenderer, MemoryLoopAnimationPartialRenderer, RustMeshAnimationPartialRenderer,
    ScriptsPartialRenderer,
};
use crate::resources::ResourcesPrerenderer;
use crate::schemas::{migrations, schema_definitions};
use crate::{admin, api, config_loader};

use crate::extension::WebExtension;

impl Extension for WebExtension {
    fn metadata(&self) -> ExtensionMetadata {
        ExtensionMetadata {
            id: "web",
            name: "Web Content & Navigation",
            version: env!("CARGO_PKG_VERSION"),
        }
    }

    fn page_data_providers(&self) -> Vec<Arc<dyn PageDataProvider>> {
        let mut providers: Vec<Arc<dyn PageDataProvider>> = vec![];

        if let Some(nav_config) = Self::navigation_config() {
            let branding = config_loader::branding_config();
            providers.push(Arc::new(
                NavigationPageDataProvider::new(nav_config).with_branding(branding),
            ));
        }

        if let Some(homepage_config) = Self::homepage_config() {
            providers.push(Arc::new(HomepagePageDataProvider::new(homepage_config)));
        }

        let docs_provider: Arc<dyn PageDataProvider> = Arc::new(DocsPageDataProvider::new());
        providers.extend([
            docs_provider,
            Arc::new(BlogListPageDataProvider::new()),
            Arc::new(BlogPostPageDataProvider::new()),
        ]);
        providers
    }

    fn content_data_providers(&self) -> Vec<Arc<dyn ContentDataProvider>> {
        vec![Arc::new(DocsContentDataProvider::new())]
    }

    fn page_prerenderers(&self) -> Vec<Arc<dyn PagePrerenderer>> {
        let mut prerenderers: Vec<Arc<dyn PagePrerenderer>> = vec![];

        if let Some(config) = Self::homepage_config() {
            prerenderers.push(Arc::new(HomepagePrerenderer::new(config)));
        }

        prerenderers.push(Arc::new(ResourcesPrerenderer));


        prerenderers
    }

    fn component_renderers(&self) -> Vec<Arc<dyn ComponentRenderer>> {
        vec![
            Arc::new(HeadAssetsPartialRenderer),
            Arc::new(HeaderPartialRenderer),
            Arc::new(FooterPartialRenderer),
            Arc::new(ScriptsPartialRenderer),
            Arc::new(CliRemoteAnimationPartialRenderer),
            Arc::new(RustMeshAnimationPartialRenderer),
            Arc::new(MemoryLoopAnimationPartialRenderer),
            Arc::new(AgenticMeshAnimationPartialRenderer),
            Arc::new(ArchitectureDiagramPartialRenderer),
        ]
    }

    fn template_data_extenders(&self) -> Vec<Arc<dyn TemplateDataExtender>> {
        vec![Arc::new(OrgUrlExtender::new())]
    }

    fn schemas(&self) -> Vec<SchemaDefinition> {
        schema_definitions()
    }

    fn migrations(&self) -> Vec<Migration> {
        migrations()
    }

    fn seeds(&self) -> Vec<Seed> {
        vec![Seed::new(
            "admin_oauth_client",
            include_str!("../schema/seeds/admin_oauth_client.sql"),
        )]
    }

    fn dependencies(&self) -> Vec<&'static str> {
        vec!["content", "users", "authz"]
    }

    fn cross_extension_tables(&self) -> Vec<&'static str> {
        vec!["markdown_content", "users"]
    }

    fn router(&self, ctx: &dyn ExtensionContext) -> Option<ExtensionRouter> {
        use axum::routing::post;

        let db_handle = ctx.database();
        let db = db_handle.as_any().downcast_ref::<Database>()?;
        let pool = db.pool()?;
        let write_pool = db.write_pool_arc().unwrap_or_else(|e| {
            tracing::warn!(error = %e, "Failed to get write pool, falling back to read pool");
            Arc::clone(&pool)
        });

        let dbpool = Arc::new(Database::from_pools(
            Arc::clone(&pool),
            Some(Arc::clone(&write_pool)),
        ));
        let (session_service, analytics_provider) = Self::build_session_service(&dbpool)?;

        let admin_api = admin::admin_router(Arc::clone(&pool));
        let webhook_api = crate::governance::hooks_webhook_router(
            Arc::clone(&write_pool),
            Arc::clone(&session_service),
            Arc::clone(&analytics_provider),
        );
        let pi_api = crate::pi::pi_terminal_router(
            Arc::clone(&write_pool),
            Arc::clone(&session_service),
            analytics_provider,
        );
        let secrets_api = admin::secrets_router(Arc::clone(&write_pool));
        let share_api = admin::share_manifest_router(Arc::clone(&pool));
        let site_md_api = admin::site_markdown_router(Arc::clone(&pool));
        let links_router = api::router(Arc::clone(&pool), self.validated_config.clone());

        let api_router = Router::new()
            .route(
                "/auth/session",
                post(api::auth::set_session).delete(api::auth::clear_session),
            )
            .merge(links_router)
            .merge(webhook_api)
            .merge(secrets_api)
            .nest("/admin", admin_api);

        let admin_dir = std::env::current_dir()
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "Failed to get current directory, using fallback");
                std::path::PathBuf::from(".")
            })
            .join("storage")
            .join("files")
            .join("admin");
        let branding = config_loader::branding_config();
        let engine = match admin::templates::AdminTemplateEngine::new(&admin_dir) {
            Ok(engine) => engine.with_branding(branding),
            Err(e) => {
                tracing::error!(error = %e, "Failed to initialize admin template engine");
                return Some(ExtensionRouter::public(api_router, "/api/public"));
            },
        };
        let bridge_auth_router = admin::bridge_auth_ssr_router(Arc::clone(&pool), engine.clone());
        let trace_router = admin::trace_ssr_router(Arc::clone(&pool), engine.clone());
        let ssr_router = admin::admin_ssr_router(pool, engine);

        let combined = Self::admin_redirects()
            .nest_service("/admin", ssr_router)
            .merge(trace_router)
            .nest_service("/bridge-auth", bridge_auth_router)
            .merge(share_api)
            .merge(site_md_api)
            .nest("/api/public", api_router)
            .merge(pi_api);

        Some(ExtensionRouter::public(combined, "/"))
    }

    fn site_auth(&self) -> Option<SiteAuthConfig> {
        Some(SiteAuthConfig {
            login_path: "/admin/login",
            protected_prefixes: &["/admin", "/bridge-auth"],
            public_prefixes: &["/admin/login", "/admin/add-passkey"],
            required_scope: "user",
        })
    }

    fn jobs(&self) -> Vec<Arc<dyn Job>> {
        vec![
            Arc::new(ContentIngestionJob),
            Arc::new(CopyExtensionAssetsJob),
            Arc::new(ContentPrerenderJob),
            Arc::new(SitemapGenerationJob),
            Arc::new(LlmsTxtGenerationJob),
            Arc::new(RobotsTxtGenerationJob),
            Arc::new(PublishPipelineJob),
            Arc::new(GovernanceBootstrapJob),
            Arc::new(ContentAnalyticsAggregationJob),
            Arc::new(BundleAdminCssJob),
        ]
    }

    fn priority(&self) -> u32 {
        100
    }

    fn config_prefix(&self) -> Option<&str> {
        Some(Self::PREFIX)
    }

    fn declares_assets(&self) -> bool {
        true
    }

    fn required_assets(&self, paths: &dyn AssetPaths) -> Vec<AssetDefinition> {
        web_assets(paths)
    }
}

impl WebExtension {
    fn admin_redirects() -> Router {
        use axum::response::Redirect;
        use axum::routing::get;

        Router::new()
            .route(
                "/login",
                get(|| async { Redirect::temporary("/admin/login") }),
            )
            .route(
                "/register",
                get(|| async { Redirect::temporary("/admin/register") }),
            )
            .route(
                "/onboarding",
                get(|| async { Redirect::temporary("/admin/continue") }),
            )
    }

    // Why: the governance webhook needs the analytics provider as well as the
    // session service — it attests the session id its callers claim — so both
    // come out of one construction rather than building the provider twice.
    fn build_session_service(
        dbpool: &Arc<Database>,
    ) -> Option<(Arc<SessionCreationService>, Arc<dyn AnalyticsProvider>)> {
        let user = UserService::new(dbpool)
            .map_err(|e| tracing::error!(error = %e, "Failed to build user service"))
            .ok()?;
        let analytics = AnalyticsService::new(dbpool, None, None)
            .map_err(|e| tracing::error!(error = %e, "Failed to build analytics service"))
            .ok()?;
        let analytics: Arc<dyn AnalyticsProvider> = Arc::new(analytics);
        Some((
            Arc::new(SessionCreationService::new(
                Arc::clone(&analytics),
                Arc::new(user),
            )),
            analytics,
        ))
    }
}
