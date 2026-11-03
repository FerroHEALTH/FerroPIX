// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `ferropix-console` binary: a thin entry point over the library run
//! path.

fn main() -> std::process::ExitCode {
    ferropix_console::command::RUN.run(std::env::args(), ferropix_console::command::dispatch)
}
