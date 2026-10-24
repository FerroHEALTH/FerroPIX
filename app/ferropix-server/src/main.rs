// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `ferropix` binary: a thin entry point over the library run path.

fn main() -> std::process::ExitCode {
    ferropix_server::run(std::env::args())
}
