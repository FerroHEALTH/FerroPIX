// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Whether a type implements `Display`, asked of a concrete type by method
//! resolution: the inherent method applies when the bound holds, and the
//! trait's otherwise (the Rust Reference, "Method call expressions").

use std::fmt::Display;
use std::marker::PhantomData;

/// The probe of the type `T`.
pub(crate) struct Probe<T>(pub(crate) PhantomData<T>);

/// The answer for a type without `Display`.
pub(crate) trait NoDisplay {
    /// Returns `false`: the type has no `Display`.
    fn displays(&self) -> bool;
}

impl<T> NoDisplay for Probe<T> {
    fn displays(&self) -> bool {
        let Probe(_marker) = self;
        false
    }
}

impl<T: Display> Probe<T> {
    /// Returns `true`: the type has `Display`.
    #[expect(dead_code, reason = "called only when a type under test has Display")]
    pub(crate) fn displays(&self) -> bool {
        let Probe(_marker) = self;
        true
    }
}

/// Whether the type `$type` implements `Display`.
macro_rules! displays {
    ($type:ty) => {{
        use $crate::display::NoDisplay as _;
        $crate::display::Probe::<$type>(::std::marker::PhantomData).displays()
    }};
}

pub(crate) use displays;
