//! The smallest Eario application: one page, one piece of text.
//!
//! Run with `cargo run -p hello-dashboard`.

use eario::prelude::*;

fn main() -> eario::Result<()> {
    Dashboard::new("Hello, Eario")
        .page("Welcome", |page| {
            page.row([text(
                "This page is defined by a few lines of Rust and served from your own machine.\n\n\
                 Choose Quit in the header to stop the application.",
            )
            .title("It works")])
        })
        .run()
}
