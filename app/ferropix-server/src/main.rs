// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `ferropix` binary: the run path of `ferrohealth-server` over the
//! library's dispatch.

fn main() -> std::process::ExitCode {
    ferropix_server::RUN.run(std::env::args(), ferropix_server::dispatch)
}
