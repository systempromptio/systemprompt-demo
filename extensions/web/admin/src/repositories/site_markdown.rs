//! Queries behind the public page-markdown endpoint (`/index.md`, `/md/…`).
//!
//! Reads the same `markdown_content` rows the prerendered HTML pages are built
//! from, so the markdown a fetcher sees is exactly as current as the site
//! itself. Only `public` rows are reachable.

use sqlx::PgPool;
use systemprompt::identifiers::SourceId;

#[derive(Debug)]
pub(crate) struct SitePageRow {
    pub source_id: SourceId,
    pub slug: String,
    pub title: String,
    pub description: String,
}

pub(crate) async fn list_public_pages(pool: &PgPool) -> Result<Vec<SitePageRow>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT source_id as "source_id: SourceId", slug, title, description
        FROM markdown_content
        WHERE public = true
          AND slug != ''
          AND slug != 'index'
        ORDER BY source_id, slug
        "#
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| SitePageRow {
            source_id: r.source_id,
            slug: r.slug,
            title: r.title,
            description: r.description,
        })
        .collect())
}

#[derive(Debug)]
pub(crate) struct PageMarkdownRow {
    pub title: String,
    pub description: String,
    pub body: String,
}

pub(crate) async fn find_page_markdown(
    pool: &PgPool,
    source_id: &SourceId,
    slug: &str,
) -> Result<Option<PageMarkdownRow>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT title, description, body
        FROM markdown_content
        WHERE public = true
          AND source_id = $1
          AND slug = $2
        "#,
        source_id.as_str(),
        slug
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| PageMarkdownRow {
        title: r.title,
        description: r.description,
        body: r.body,
    }))
}
