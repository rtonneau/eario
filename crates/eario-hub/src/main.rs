//! The `eario-hub` executable. See the crate documentation of `eario_hub`.

use std::process::ExitCode;

fn main() -> ExitCode {
    match eario_hub::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error} ({})", error.code());
            ExitCode::FAILURE
        }
    }
}
