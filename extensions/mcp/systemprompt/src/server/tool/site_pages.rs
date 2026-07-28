//! `list_site_pages` / `fetch_site_page` — the live half of the docs hub.
//!
//! Where `list_topics`/`get_topic` answer from markdown compiled into this
//! binary, these two fetch the site's own page-markdown endpoint (`/index.md`,
//! `/md/{section}/{slug}.md` — see `handlers/site_markdown.rs` in the web
//! extension) over HTTP, so a terminal session reads what the site publishes
//! *now* rather than what it published at build time.
//!
//! This is deliberate, allowed egress, sitting next to `fetch_remote_docs`,
//! which exists to be refused — and the contrast is the point. That tool takes
//! a caller-controlled path to an internet this deployment forbids. This one
//! cannot be steered anywhere: the input is a `{section, slug}` pair, the
//! section is a two-value enum, the slug has to survive [`valid_slug`], and the
//! base URL is fixed at process start ([`SITE_BASE_URL_ENV`], defaulting to the
//! public site). There is no input for which [`site_page_url`] yields a URL on
//! another host, so the no-SSRF property is by construction rather than by
//! filter.

use rmcp::ErrorData as McpError;
use systemprompt::identifiers::McpExecutionId;
use systemprompt::mcp::McpToolHandler;
use systemprompt::models::artifacts::CliArtifact;
use systemprompt::models::execution::context::RequestContext as SysRequestContext;

use crate::tools::{FetchSitePageInput, ListSitePagesInput, SitePageSection};

use super::text_artifact;

// Why: overrides where the site lives, for deployments whose MCP hub should
// read a staging or local copy; read once per call, never caller-influenced.
pub(crate) const SITE_BASE_URL_ENV: &str = "SYSTEMPROMPT_SITE_BASE_URL";

const FALLBACK_SITE_BASE_URL: &str = "https://systemprompt.io";

const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

// Why: two limits for two resources — MAX_RESPONSE_BYTES guards this
// process's memory, MAX_MODEL_CHARS guards the model's context.
const MAX_RESPONSE_BYTES: usize = 256 * 1024;

pub(crate) const MAX_MODEL_CHARS: usize = 20_000;

// Why: env beats profile beats public site — so a local deployment reads its
// own pages by default, and only a deployment with no profile at all falls
// back to the published site.
fn site_base_url() -> String {
    let base = std::env::var(SITE_BASE_URL_ENV).unwrap_or_else(|_| {
        systemprompt::config::ProfileBootstrap::get().map_or_else(
            |_| FALLBACK_SITE_BASE_URL.to_owned(),
            |profile| profile.server.api_internal_url.clone(),
        )
    });
    base.trim_end_matches('/').to_owned()
}

// Why: a slug as the site's ingestion pipeline mints them — `/`-separated
// segments of lowercase alphanumerics and hyphens. This is the entire input
// surface of `fetch_site_page`, so everything path traversal could be spelled
// with — `..`, `%`, `#`, `?`, leading `/` — fails here.
#[must_use]
pub(crate) fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 200
        && slug.split('/').all(|segment| {
            !segment.is_empty()
                && segment
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

/// The one place a fetch URL is composed.
///
/// # Errors
/// A human-readable rejection when the slug fails `valid_slug`.
pub fn site_page_url(base: &str, section: SitePageSection, slug: &str) -> Result<String, String> {
    if !valid_slug(slug) {
        return Err(format!(
            "Invalid slug '{slug}'. A slug is lowercase alphanumerics and hyphens, with '/' \
             between nested segments — e.g. \"services/ai\". Call `list_site_pages` to see \
             every valid one."
        ));
    }
    Ok(format!(
        "{}/md/{}/{slug}.md",
        base.trim_end_matches('/'),
        section.as_str()
    ))
}

/// Clamp page markdown to what is worth putting in a model's context. Returns
/// the (possibly shortened) text and whether anything was dropped.
#[must_use]
pub fn truncate_for_model(text: &str, max_chars: usize) -> (String, bool) {
    if text.chars().count() <= max_chars {
        return (text.to_owned(), false);
    }
    (text.chars().take(max_chars).collect(), true)
}

async fn fetch_markdown(url: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .build()
        .map_err(|e| format!("Could not build an HTTP client: {e}"))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Could not reach the site at `{url}`: {e}"))?;

    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(format!(
            "The site has no page at `{url}`. Call `list_site_pages` for the current index."
        ));
    }
    if !status.is_success() {
        return Err(format!("The site answered `{url}` with HTTP {status}."));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Could not read the site's response from `{url}`: {e}"))?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(format!(
            "The page at `{url}` is larger than {MAX_RESPONSE_BYTES} bytes; refusing to read it."
        ));
    }
    String::from_utf8(bytes.to_vec())
        .map_err(|_utf8| format!("The page at `{url}` is not valid UTF-8 text."))
}

fn page_body(url: &str, markdown: &str) -> String {
    let (shown, truncated) = truncate_for_model(markdown, MAX_MODEL_CHARS);
    let mut body = format!("Fetched live from {url}\n\n---\n\n{shown}");
    if truncated {
        body.push_str(&format!(
            "\n\n---\n\n*Truncated at {MAX_MODEL_CHARS} characters; the full page is at {url}.*"
        ));
    }
    body
}

pub(in crate::server) struct ListSitePagesHandler;

impl McpToolHandler for ListSitePagesHandler {
    type Input = ListSitePagesInput;
    type Output = CliArtifact;

    fn tool_name(&self) -> &'static str {
        "list_site_pages"
    }

    fn description(&self) -> &'static str {
        "List every public page of the live systemprompt.io site."
    }

    async fn handle(
        &self,
        _input: Self::Input,
        _ctx: &SysRequestContext,
        _exec_id: &McpExecutionId,
    ) -> Result<(Self::Output, String), McpError> {
        let url = format!("{}/index.md", site_base_url());
        let markdown = fetch_markdown(&url)
            .await
            .map_err(|reason| McpError::internal_error(reason, None))?;
        let summary = format!("Live page index fetched from {url}");
        Ok((
            text_artifact("Live Site Pages", page_body(&url, &markdown)),
            summary,
        ))
    }
}

pub(in crate::server) struct FetchSitePageHandler;

impl McpToolHandler for FetchSitePageHandler {
    type Input = FetchSitePageInput;
    type Output = CliArtifact;

    fn tool_name(&self) -> &'static str {
        "fetch_site_page"
    }

    fn description(&self) -> &'static str {
        "Fetch one live systemprompt.io page as markdown, by section and slug."
    }

    async fn handle(
        &self,
        input: Self::Input,
        _ctx: &SysRequestContext,
        _exec_id: &McpExecutionId,
    ) -> Result<(Self::Output, String), McpError> {
        let url = site_page_url(&site_base_url(), input.section, &input.slug)
            .map_err(|reason| McpError::invalid_params(reason, None))?;
        let markdown = fetch_markdown(&url)
            .await
            .map_err(|reason| McpError::internal_error(reason, None))?;
        let summary = format!("Live page fetched from {url}");
        Ok((
            text_artifact(
                &format!("{}/{}", input.section.as_str(), input.slug),
                page_body(&url, &markdown),
            ),
            summary,
        ))
    }
}
