//! Decorative animation partials shared across public-site templates.
//!
//! Templates are `include_str!`-compiled into the binary, so editing them
//! requires a rebuild and restart before `just publish` will serve the new
//! markup.

use async_trait::async_trait;
use systemprompt::template_provider::{
    ComponentContext, ComponentRenderer, PartialTemplate, RenderedComponent,
};
use systemprompt::traits::ProviderError;

use super::partials::{PRIORITY_MID, static_partial};

static_partial!(
    CliRemoteAnimationPartialRenderer,
    "web:cli-remote-animation",
    "ANIMATION_CLI_REMOTE",
    "animation-cli-remote",
    "animation-cli-remote.html",
    PRIORITY_MID
);

static_partial!(
    RustMeshAnimationPartialRenderer,
    "web:rust-mesh-animation",
    "RUST_MESH_ANIMATION",
    "rust-mesh-animation",
    "animation-rust-mesh.html",
    PRIORITY_MID
);

static_partial!(
    MemoryLoopAnimationPartialRenderer,
    "web:memory-loop-animation",
    "ANIMATION_MEMORY_LOOP",
    "animation-memory-loop",
    "animation-memory-loop.html",
    PRIORITY_MID
);

static_partial!(
    AgenticMeshAnimationPartialRenderer,
    "web:agentic-mesh-animation",
    "ANIMATION_AGENTIC_MESH",
    "animation-agentic-mesh",
    "animation-agentic-mesh.html",
    PRIORITY_MID
);

static_partial!(
    ArchitectureDiagramPartialRenderer,
    "web:architecture-diagram",
    "ARCHITECTURE_DIAGRAM",
    "architecture-diagram",
    "architecture-diagram.html",
    PRIORITY_MID
);
