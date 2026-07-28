//! Built-in governance policies. Each submodule registers itself with the
//! `policy` registry via `inventory::submit!`. Adding a new policy means
//! creating a new file here and listing it below.

pub(crate) mod rate_limit;
mod scope_check;
mod secret_scan;
mod tool_blocklist;

// Why: a shared contract, not a local detail — the handler writes the prompt
// under this key and `secret_scan` reads it back to name the right location on
// a deny. Changing one side alone silently mislabels every prompt denial.
pub(crate) const PROMPT_INPUT_KEY: &str = "prompt";

// Why: a prompt submission carries no tool name, and the audit's target column
// is what an operator queries; `demo/governance/10-pi-agent` pins this value.
pub(crate) const PROMPT_TOOL_NAME: &str = "user_prompt";
