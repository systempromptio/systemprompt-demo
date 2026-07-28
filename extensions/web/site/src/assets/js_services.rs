//! Shared JavaScript service module definitions.

use std::path::Path;
use systemprompt::extension::AssetDefinition;

macro_rules! svc_js {
    ($p:expr, $name:literal) => {
        AssetDefinition::js($p.join($name), concat!("js/services/", $name))
    };
}

pub(super) fn public_js_assets(storage_js: &Path) -> Vec<AssetDefinition> {
    vec![
        AssetDefinition::js(storage_js.join("analytics.js"), "js/analytics.js"),
        AssetDefinition::js(
            storage_js.join("analytics-helpers.js"),
            "js/analytics-helpers.js",
        ),
        AssetDefinition::js(storage_js.join("docs.js"), "js/docs.js"),
        AssetDefinition::js(storage_js.join("mobile-menu.js"), "js/mobile-menu.js"),
        AssetDefinition::js(storage_js.join("terminal-demo.js"), "js/terminal-demo.js"),
        AssetDefinition::js(storage_js.join("blog-images.js"), "js/blog-images.js"),
        AssetDefinition::js(storage_js.join("hero-header.js"), "js/hero-header.js"),
        AssetDefinition::js(storage_js.join("home-scene.js"), "js/home-scene.js"),
    ]
}

pub(super) fn service_js_assets(storage_js: &Path) -> Vec<AssetDefinition> {
    let p = storage_js.join("services");
    let mut v = service_core_js(&p);
    v.extend(service_plugin_js(&p));
    v.extend(service_skill_js(&p));
    v.extend(service_webauthn_js(&p));
    v.extend(service_utils_js(storage_js));
    v
}

fn service_core_js(p: &Path) -> Vec<AssetDefinition> {
    vec![
        svc_js!(p, "api.js"),
        svc_js!(p, "auth.js"),
        svc_js!(p, "bootstrap.js"),
        svc_js!(p, "confirm.js"),
        svc_js!(p, "dropdown.js"),
        svc_js!(p, "events.js"),
        svc_js!(p, "header-actions.js"),
        svc_js!(p, "header-search.js"),
        svc_js!(p, "list-page.js"),
        svc_js!(p, "sidebar.js"),
        svc_js!(p, "table-sort.js"),
        svc_js!(p, "theme.js"),
        svc_js!(p, "sp-auth-pane.js"),
        svc_js!(p, "sp-auth-pane-auth.js"),
        svc_js!(p, "sp-auth-pane-forms.js"),
        svc_js!(p, "sp-auth-pane-governance.js"),
        svc_js!(p, "sp-auth-pane-helpers.js"),
        svc_js!(p, "sp-auth-pane-pulse.js"),
        svc_js!(p, "sp-auth-pane-stats.js"),
        svc_js!(p, "sp-auth-pane-tabs.js"),
        svc_js!(p, "sp-auth-pane-view.js"),
        svc_js!(p, "sp-pulse-admin.js"),
        svc_js!(p, "sp-confirm-dialog.js"),
        svc_js!(p, "pi-constants.js"),
        svc_js!(p, "pi-format.js"),
        svc_js!(p, "pi-transport.js"),
        svc_js!(p, "pi-replay.js"),
        svc_js!(p, "pi-terminal-view.js"),
        svc_js!(p, "pi-terminal-canned.js"),
        svc_js!(p, "pi-terminal-capacity.js"),
        svc_js!(p, "pi-terminal-dom.js"),
        svc_js!(p, "pi-terminal-frames.js"),
        svc_js!(p, "pi-terminal-gate.js"),
        svc_js!(p, "pi-terminal-input.js"),
        svc_js!(p, "pi-terminal-meters.js"),
        svc_js!(p, "pi-terminal-palette.js"),
        svc_js!(p, "pi-terminal-prose.js"),
        svc_js!(p, "pi-terminal-rail.js"),
        svc_js!(p, "pi-terminal-session.js"),
        svc_js!(p, "pi-terminal-setup.js"),
        svc_js!(p, "pi-terminal-stream.js"),
        svc_js!(p, "pi-terminal-artifacts.js"),
        svc_js!(p, "pi-artifact-overlay.js"),
        svc_js!(p, "pi-highlight.js"),
        svc_js!(p, "pi-markdown.js"),
        svc_js!(p, "pi-gate-view.js"),
        svc_js!(p, "pi-gate-parts.js"),
        svc_js!(p, "pi-gate-cards.js"),
        svc_js!(p, "pi-gate-records.js"),
        svc_js!(p, "sp-pi-terminal.js"),
        svc_js!(p, "sp-conversation-list.js"),
        svc_js!(p, "sp-toast.js"),
        svc_js!(p, "toast.js"),
        svc_js!(p, "toc-highlight.js"),
    ]
}

fn service_plugin_js(p: &Path) -> Vec<AssetDefinition> {
    vec![
        svc_js!(p, "plugin-details-ui.js"),
        svc_js!(p, "plugin-details.js"),
        svc_js!(p, "plugin-env-ui.js"),
        svc_js!(p, "plugin-env.js"),
        svc_js!(p, "plugin-resources-helpers.js"),
        svc_js!(p, "plugin-resources.js"),
    ]
}

fn service_skill_js(p: &Path) -> Vec<AssetDefinition> {
    vec![svc_js!(p, "skill-files.js")]
}

fn service_webauthn_js(p: &Path) -> Vec<AssetDefinition> {
    vec![
        svc_js!(p, "webauthn-helpers.js"),
        svc_js!(p, "webauthn-login.js"),
        svc_js!(p, "webauthn-login-ui.js"),
        svc_js!(p, "webauthn-passkey.js"),
        svc_js!(p, "webauthn-passkey-helpers.js"),
        svc_js!(p, "webauthn-utils.js"),
    ]
}

fn service_utils_js(storage_js: &Path) -> Vec<AssetDefinition> {
    vec![
        AssetDefinition::js(storage_js.join("utils/dom.js"), "js/utils/dom.js"),
        AssetDefinition::js(storage_js.join("utils/format.js"), "js/utils/format.js"),
        AssetDefinition::js(storage_js.join("utils/form.js"), "js/utils/form.js"),
    ]
}

pub(super) fn admin_assets(storage_css: &Path, _storage_js: &Path) -> Vec<AssetDefinition> {
    vec![AssetDefinition::css(
        storage_css.join("admin-bundle.css"),
        "css/admin-bundle.css",
    )]
}
