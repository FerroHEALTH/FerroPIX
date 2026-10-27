// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The console's messages, each naming one message of the catalog by its
//! id.
//!
//! The build script writes the variants from the English catalog, each
//! documented by its message's comment there. No specification governs the
//! catalog layout: our own design.

/// Declares the console's messages: each variant names one message of the
/// catalog by its id.
macro_rules! messages {
    ($($(#[$meta:meta])+ $name:ident = $id:literal,)+) => {
        /// A string the console draws, by its message in the catalog.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Msg {
            $($(#[$meta])+ $name,)+
        }

        impl Msg {
            /// Every message, in declaration order.
            pub const ALL: &[Self] = &[$(Self::$name,)+];

            /// The message's id in the catalog.
            #[must_use]
            pub const fn id(self) -> &'static str {
                match self {
                    $(Self::$name => $id,)+
                }
            }
        }
    };
}

include!(concat!(env!("OUT_DIR"), "/messages.rs"));
