//! Putting the marketplace's skills where a pi session can load them.
//!
//! The same four skills reach three surfaces — Claude Desktop and Cowork
//! through the exported plugin bundle, and this terminal — and all three read
//! the Agent Skills on-disk shape: a directory holding a `SKILL.md` whose
//! frontmatter carries `name` and `description`. `services/skills/<id>/` splits
//! that in two, keeping the metadata in `config.yaml` so the rest of the
//! platform can read it as configuration, so the frontmatter is synthesised
//! here rather than stored twice and allowed to drift.
//!
//! Written per session, into the session's own workspace. Nothing is shared
//! between conversations, and the directory dies with the workspace.
//!
//! **`--no-skills` stays on.** pi's `--skill <path>` is additive even with
//! discovery disabled, so the session loads exactly the skills written here and
//! nothing from the host, the project tree, or an installed package. That is
//! the whole reason skills can be enabled at all without widening what a
//! session can see.

use std::path::{Path, PathBuf};

use systemprompt::config::ProfileBootstrap;
use systemprompt::models::AppPaths;

#[derive(Debug, serde::Serialize)]
pub(super) struct SkillCommand {
    pub(super) command: String,
    pub(super) description: String,
}

pub(super) async fn catalogue() -> Vec<SkillCommand> {
    read_all(&skills_dir())
        .await
        .into_iter()
        .map(|skill| SkillCommand {
            command: format!("/skill:{}", skill.slug),
            description: skill.description,
        })
        .collect()
}

struct Skill {
    slug: String,
    description: String,
    body: String,
}

pub(super) async fn materialise(workspace: &Path) -> Option<PathBuf> {
    let source = skills_dir();
    let skills = read_all(&source).await;
    if skills.is_empty() {
        tracing::warn!(
            source = %source.display(),
            "no pi skills were readable; the session starts without any"
        );
        return None;
    }

    let root = workspace.join(".pi").join("skills");
    let mut written = 0usize;
    for skill in &skills {
        let dir = root.join(&skill.slug);
        if let Err(e) = write_one(&dir, skill).await {
            tracing::warn!(skill = %skill.slug, error = %e, "could not write a pi skill");
            continue;
        }
        written += 1;
    }

    (written > 0).then_some(root)
}

async fn write_one(dir: &Path, skill: &Skill) -> std::io::Result<()> {
    tokio::fs::create_dir_all(dir).await?;
    let front = format!(
        "---\nname: \"{}\"\ndescription: \"{}\"\n---\n\n",
        skill.slug,
        escape(&skill.description)
    );
    tokio::fs::write(dir.join("SKILL.md"), front + &skill.body).await
}

async fn read_all(source: &Path) -> Vec<Skill> {
    let Ok(mut entries) = tokio::fs::read_dir(source).await else {
        return Vec::new();
    };
    let mut skills = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let dir = entry.path();
        match read_one(&dir).await {
            Ok(Some(skill)) => skills.push(skill),
            Ok(None) => {},
            Err(e) => tracing::warn!(dir = %dir.display(), error = %e, "skipping a pi skill"),
        }
    }
    skills.sort_by(|a, b| a.slug.cmp(&b.slug));
    skills
}

async fn read_one(dir: &Path) -> Result<Option<Skill>, String> {
    let config = dir.join("config.yaml");
    if !tokio::fs::try_exists(&config).await.unwrap_or(false) {
        return Ok(None);
    }
    let raw = tokio::fs::read_to_string(&config)
        .await
        .map_err(|e| e.to_string())?;

    if scalar(&raw, "enabled").as_deref() == Some("false") {
        return Ok(None);
    }

    let id = scalar(&raw, "id").ok_or_else(|| "no `id:`".to_owned())?;
    let description = scalar(&raw, "description").ok_or_else(|| "no `description:`".to_owned())?;
    let file = scalar(&raw, "file").unwrap_or_else(|| "SKILL.md".to_owned());
    let body = tokio::fs::read_to_string(dir.join(&file))
        .await
        .map_err(|e| format!("{file}: {e}"))?;

    Ok(Some(Skill {
        slug: id.replace('_', "-"),
        description,
        body,
    }))
}

/// Pull one top-level scalar out of a flat skill `config.yaml`.
///
/// Not a YAML parse on purpose. These files are flat by construction — the
/// services loader rejects unknown keys — and a parser here would give a
/// malformed nested value the power to fail a spawn, which is exactly the
/// coupling this module avoids everywhere else.
pub fn scalar(raw: &str, key: &str) -> Option<String> {
    raw.lines()
        .find_map(|line| line.strip_prefix(&format!("{key}:")))
        .map(|value| value.trim().trim_matches(['"', '\'']).to_owned())
        .filter(|value| !value.is_empty())
}

pub fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn skills_dir() -> PathBuf {
    ProfileBootstrap::get()
        .map_err(|e| e.to_string())
        .and_then(|profile| AppPaths::from_profile(&profile.paths).map_err(|e| e.to_string()))
        .map_or_else(
            |_| PathBuf::from("./services/skills"),
            |paths| paths.system().services().join("skills"),
        )
}
