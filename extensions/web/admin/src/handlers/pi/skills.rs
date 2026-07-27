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

/// One entry in the `/` palette the widget renders.
#[derive(Debug, serde::Serialize)]
pub(super) struct SkillCommand {
    /// What a viewer types, including the `/skill:` prefix pi expects.
    pub(super) command: String,
    pub(super) description: String,
}

/// The skills a session will have, for the widget's slash-command palette.
///
/// Read from the same source [`materialise`] writes from, rather than asking
/// the child. pi does expose a `get_commands` RPC, but answering it here would
/// mean correlating a response frame back to a waiting HTTP request — a
/// mechanism the pump does not have and would exist solely for a dropdown.
/// The server already knows what it wrote.
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

/// A skill as the two files on disk describe it.
struct Skill {
    /// Directory name and frontmatter `name`. pi validates this against
    /// `a-z0-9-` and rejects anything else, so the underscored config id is
    /// converted rather than passed through.
    slug: String,
    description: String,
    body: String,
}

/// Write every enabled skill under `<workspace>/.pi/skills/` and return that
/// directory, or `None` when there is nothing to write.
///
/// Best-effort by design: a session with no skills is a working session, and
/// failing the spawn because one `config.yaml` is malformed would take the
/// terminal down for a content mistake. Every skipped skill is logged, because
/// a silently missing skill looks to a viewer like the feature not existing.
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
    // A description is the one required field pi will not infer: a skill
    // missing one is dropped silently, which is the failure mode this whole
    // function exists to avoid.
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

/// Read one `services/skills/<id>/` directory.
///
/// `Ok(None)` is "not a skill directory" — anything without a `config.yaml`,
/// including a stray file. `Err` is a directory that looks like a skill and is
/// not usable, which is worth a log line.
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
fn scalar(raw: &str, key: &str) -> Option<String> {
    raw.lines()
        .find_map(|line| line.strip_prefix(&format!("{key}:")))
        .map(|value| value.trim().trim_matches(['"', '\'']).to_owned())
        .filter(|value| !value.is_empty())
}

/// Make a description safe inside a double-quoted YAML scalar.
fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// `services/skills/` under the active profile, matching how
/// [`super::config`] resolves `services/config/pi.yaml`.
fn skills_dir() -> PathBuf {
    ProfileBootstrap::get()
        .map_err(|e| e.to_string())
        .and_then(|profile| AppPaths::from_profile(&profile.paths).map_err(|e| e.to_string()))
        .map_or_else(
            |_| PathBuf::from("./services/skills"),
            |paths| paths.system().services().join("skills"),
        )
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "assertions in tests")]
mod tests {
    use super::{escape, scalar};

    #[test]
    fn reads_quoted_and_bare_scalars() {
        let raw = "id: demonstrate_governance\ndescription: \"Exercise the pipeline\"\n";
        assert_eq!(scalar(raw, "id").as_deref(), Some("demonstrate_governance"));
        assert_eq!(
            scalar(raw, "description").as_deref(),
            Some("Exercise the pipeline")
        );
        assert_eq!(scalar(raw, "missing"), None);
    }

    /// A key that only appears nested must not be mistaken for a top-level one
    /// — `tags:` entries are indented, and a naive `contains` would match.
    #[test]
    fn ignores_indented_keys() {
        assert_eq!(scalar("tags:\n  id: nope\n", "id"), None);
    }

    /// An unescaped quote in a description would produce frontmatter pi cannot
    /// parse, and pi drops a skill with no readable description silently.
    #[test]
    fn escapes_quotes_in_a_description() {
        assert_eq!(escape(r#"the "hub" tool"#), r#"the \"hub\" tool"#);
    }

    /// Every shipped skill must survive the on-disk round trip, because the
    /// failure mode is a slash-command that simply is not there.
    #[test]
    fn the_shipped_skills_all_have_what_pi_requires() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../services/skills");
        let entries = std::fs::read_dir(root).expect("services/skills is readable");
        let mut seen = 0;
        for entry in entries.flatten() {
            let config = entry.path().join("config.yaml");
            if !config.exists() {
                continue;
            }
            let raw = std::fs::read_to_string(&config).expect("config.yaml is readable");
            let id = scalar(&raw, "id").expect("every skill has an id");
            assert!(
                scalar(&raw, "description").is_some(),
                "{id} has no description; pi would drop it without saying so"
            );
            let slug = id.replace('_', "-");
            assert!(
                slug.len() <= 64
                    && slug
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{slug} is not a name pi will accept"
            );
            seen += 1;
        }
        assert!(seen > 0, "no skills found under {root}");
    }
}
