//! Copies the public-site partial templates into `OUT_DIR` so `partials.rs`
//! can `include_str!` them crate-relatively. The sources live in
//! `services/web/templates/partials/` (config, per repo layout rules); without
//! this hop every embed would reach four directory levels out of the crate,
//! pinning the crate to one repo layout.

use std::error::Error;
use std::path::Path;

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")?;
    let out_dir = std::env::var("OUT_DIR")?;

    let src = Path::new(&manifest_dir).join("../../../services/web/templates/partials");
    let dst = Path::new(&out_dir).join("partials");
    std::fs::create_dir_all(&dst)?;

    println!("cargo:rerun-if-changed={}", src.display());
    for entry in std::fs::read_dir(&src)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "html") {
            std::fs::copy(&path, dst.join(entry.file_name()))?;
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    Ok(())
}
