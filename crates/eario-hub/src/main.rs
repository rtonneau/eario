//! The `eario-hub` executable. See the crate documentation of `eario_hub`
//! and, for its commands, `eario_hub::cli`.

use std::process::ExitCode;

fn main() -> ExitCode {
    eario_hub::cli::main(std::env::args_os().skip(1))
}
