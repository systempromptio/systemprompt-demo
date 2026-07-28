//! The public page-markdown endpoint: the site, as an agent reads it.
//!
//! Two routes. `GET /index.md` lists every public page as the exact path a
//! fetcher should request next; `GET /md/{section}/{slug}.md` returns that
//! page's source markdown. The `/md/` prefix exists because the HTML pages own
//! `/documentation/{slug}` and `/blog/{slug}` — a wildcard `.md` route beside
//! them would shadow the prerendered site.
//!
//! This is the site half of the terminal's live-content bridge: the
//! `systemprompt` MCP hub's `fetch_site_page` tool builds URLs of exactly this
//! shape from a `{section, slug}` pair, so what an agent can reach is decided
//! here — by what parses — rather than by whatever URL a model composes.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use sqlx::PgPool;

use crate::repositories::site_markdown as repo;

/// The `source_id` values a path may name. Everything else 404s, including
/// values that exist in the DB but are not meant to be listed here.
const SECTIONS: &[&str] = &["documentation", "blog"];

const CACHE_CONTROL: &str = "public, max-age=300";
const CONTENT_TYPE: &str = "text/markdown; charset=utf-8";

/// Split a `{section}/{slug}.md` wildcard into its validated halves.
///
/// Slugs are the ingestion pipeline's: lowercase alphanumerics and hyphens,
/// `/`-separated segments for nested docs (`services/ai`). Anything outside
/// that grammar — `..`, absolute paths, uppercase, empty segments — is `None`,
/// which the handler turns into the same 404 a missing page gets.
pub fn parse_md_path(path: &str) -> Option<(&str, &str)> {
    let path = path.strip_suffix(".md")?;
    let (section, slug) = path.split_once('/')?;
    if !SECTIONS.contains(&section) {
        return None;
    }
    if slug.len() > 200 || !slug.split('/').all(valid_segment) {
        return None;
    }
    Some((section, slug))
}

fn valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

// lint-ok: http-error — this endpoint speaks text/markdown, not the admin
// JSON error shape; every status is hand-picked (200/404/500) on purpose
fn markdown_response(body: String) -> Response {
    (
        [
            (header::CONTENT_TYPE, CONTENT_TYPE),
            (header::CACHE_CONTROL, CACHE_CONTROL),
        ],
        body,
    )
        .into_response()
}

// lint-ok: http-error — a plain 404 is this surface's whole error vocabulary
fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "Not found.\n").into_response()
}

// lint-ok: http-error — serves raw markdown; the admin error types are JSON
pub(crate) async fn index_handler(State(pool): State<Arc<PgPool>>) -> Response {
    let pages = match repo::list_public_pages(&pool).await {
        Ok(pages) => pages,
        Err(e) => {
            tracing::error!(error = %e, "site markdown index query failed");
            // lint-ok: http-error — logged above; a markdown client gets plain text
            return (StatusCode::INTERNAL_SERVER_ERROR, "Unavailable.\n").into_response();
        },
    };

    let mut body = String::from(
        "# systemprompt.io — page index\n\n\
         Every public page of this site, as source markdown. Fetch any entry \
         at its listed path.\n",
    );
    let mut current_section = "";
    for page in &pages {
        let section = page.source_id.as_str();
        if section != current_section {
            current_section = section;
            body.push_str(&format!("\n## {current_section}\n\n"));
        }
        body.push_str(&format!(
            "- [/md/{section}/{slug}.md](/md/{section}/{slug}.md) — **{}** — {}\n",
            page.title,
            page.description,
            slug = page.slug
        ));
    }
    markdown_response(body)
}

pub(crate) async fn page_handler(
    State(pool): State<Arc<PgPool>>,
    Path(path): Path<String>,
    // lint-ok: http-error — serves raw markdown; the admin error types are JSON
) -> Response {
    let Some((section, slug)) = parse_md_path(&path) else {
        return not_found();
    };
    let source_id = systemprompt::identifiers::SourceId::new(section.to_owned());
    match repo::find_page_markdown(&pool, &source_id, slug).await {
        Ok(Some(page)) => markdown_response(format!(
            "# {}\n\n{}\n\n{}\n",
            page.title,
            page.description,
            page.body.trim_end()
        )),
        Ok(None) => not_found(),
        Err(e) => {
            tracing::error!(error = %e, "site markdown page query failed");
            // lint-ok: http-error — logged above; a markdown client gets plain text
            (StatusCode::INTERNAL_SERVER_ERROR, "Unavailable.\n").into_response()
        },
    }
}
