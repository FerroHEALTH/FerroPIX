// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Rebuilds the console when a catalog file is added to or removed from
//! `locales/`: the catalog's loader embeds every file it finds there, and
//! the compiler tracks only the files it embedded.

fn main() {
    println!("cargo::rerun-if-changed=locales");
}
