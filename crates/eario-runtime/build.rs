//! Makes sure the directory embedded by `src/assets.rs` exists.
//!
//! The frontend is built by `cargo xtask frontend` into `frontend-dist/`.
//! On a checkout where that has not been done yet, a placeholder page is
//! written instead, so `cargo check` and `cargo test` work without Node.js.
//! The published crate ships the real build and never takes this path.

use std::fs;
use std::path::Path;

const PLACEHOLDER: &str = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <title>Eario: frontend not built</title>
  </head>
  <body style="font-family: system-ui, sans-serif; margin: 3rem; max-width: 40rem">
    <h1>The Eario frontend has not been built</h1>
    <p>
      This executable was compiled from a source checkout without the frontend.
      Run <code>cargo xtask frontend</code> at the root of the Eario repository,
      then build again.
    </p>
  </body>
</html>
"#;

fn main() {
    println!("cargo:rerun-if-changed=frontend-dist");
    println!("cargo:rerun-if-changed=build.rs");

    let dist = Path::new("frontend-dist");
    let index = dist.join("index.html");
    if index.exists() {
        return;
    }
    println!(
        "cargo:warning=frontend-dist/index.html is missing; embedding a placeholder page. \
         Run `cargo xtask frontend` to build the real frontend."
    );
    if let Err(error) = fs::create_dir_all(dist).and_then(|()| fs::write(&index, PLACEHOLDER)) {
        panic!("could not write the placeholder frontend to {}: {error}", index.display());
    }
}
