# Forking this repo for a new product

Every brand-bearing surface loads from configuration; no product identity is
compiled into Rust. To rebrand a fork, edit these and nothing else:

| Surface | File |
|---------|------|
| Site name, domain, titles, support email, logos, colors | `services/web/config/theme.yaml` (the `CUSTOMIZE THESE` block) |
| Homepage hero and section copy | `services/web/config/homepage.yaml` |
| Demo showcase pillars/categories and their feature URLs | `services/web/config/demo-scanner.yaml` |
| Blog base URL | `services/config/blog.yaml` |
| Package metadata (authors, homepage, repository, description) | root `Cargo.toml` `[workspace.package]` |
| Published image name | `.github/workflows/docker.yml` `IMAGE`, then every consumer — `scripts/check-image-name.sh` fails until they agree |
| Outbound email identity | the `site_url` / `admin_notify_email` secrets (unset, email falls back to the deployment's own `api_external_url` and skips reviewer notices) |

What deliberately keeps the upstream name:

- `github.com/systempromptio/systemprompt-template` URLs — the source repo,
  not the brand; retarget when you rehost.
- `docs/` and `services/content/` prose — product documentation you will be
  rewriting anyway.
- `demo/recording/` asciicast/SVG assets — baked recordings; re-record to
  rebrand.
- The MCP docs corpus under `extensions/mcp/systemprompt/content/` — embedded
  by design so that binary is self-contained; edit the markdown there.

`scripts/check-image-name.sh` and `scripts/check-fork-drift.sh` are the two
gates that keep a fork honest about what it changed.
