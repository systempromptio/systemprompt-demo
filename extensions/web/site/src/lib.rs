//! Public-site page data providers for the web extension.
//!
//! Each module owns the data model for a section of the public site
//! and exposes a `*PageDataProvider` that the core SSR runtime calls when
//! rendering.
//!
//! - [`navigation`] — header / footer nav config consumed by every page.
//! - [`partials`] / `partials_animations` — shared template fragments.
//! - [`extenders`] — URL extenders that splice org-specific routes onto the
//!   public surface.
//! - [`assets`] — `web_assets()` enumerates the static asset manifest for the
//!   extension trait.

pub mod assets;
pub mod extenders;
pub mod navigation;
pub mod partials;
mod partials_animations;
pub mod resources;

pub use assets::web_assets;
