// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The matching configuration in effect, `GET {base}/operator/matching`: the
//! thresholds, automatic linking, the blocking keys and the weight of every
//! field the scorer compares.

use serde::{Deserialize, Serialize};

/// What `GET {base}/operator/matching` answers with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Matching {
    /// The score at or above which a pair grades certain.
    pub link_at: f64,
    /// The score at or above which a pair grades probable and is queued.
    pub review_at: f64,
    /// The score at or above which a pair grades possible.
    pub possible_at: f64,
    /// Whether a certain pair is linked without a steward.
    pub auto_link: bool,
    /// The blocking keys, in the configured order, as `[matching]` names
    /// them.
    pub blocking_keys: Vec<String>,
    /// Every field the scorer compares, in its order.
    pub fields: Vec<FieldWeights>,
}

/// The weights of one field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldWeights {
    /// The field, as `[matching.fields]` names it.
    pub field: String,
    /// The probability the field agrees on a true match.
    pub m: f64,
    /// The probability the field agrees on a non-match.
    pub u: f64,
    /// Whether the weights come from `[matching.fields]` rather than the
    /// built-in defaults.
    pub configured: bool,
}
