// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The test devices the Nuts tests run against: a Nuts node with its
//! authorization server, the `DPoP` proof check it holds requests to, and
//! synthetic keys. They came from FerroFED's testkit with the crate, since a
//! dev-dependency on that testkit cannot resolve here. No specification
//! governs the devices: our own design.

pub(crate) mod dpop;
pub(crate) mod keys;
pub(crate) mod mock;
pub(crate) mod nuts;
