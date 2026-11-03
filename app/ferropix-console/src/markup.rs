// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The shorthands the pages write their own markup with beside FerroUI's
//! parts.
//!
//! As FerroUI's gallery writes it: markup made here from escaped parts, and
//! text escaped for it. No specification governs this: our own design.

use ferrohealth_ui::html::Html;

/// `part` when there is one, else no markup.
#[must_use]
pub fn opt(part: Option<Html>) -> Html {
    part.unwrap_or_default()
}

/// A hidden form field `name` holding `value`.
#[must_use]
pub fn hidden(name: &str, value: &str) -> Html {
    Html::raw(format!(
        "<input type=\"hidden\" name=\"{}\" value=\"{}\">",
        Html::text(name),
        Html::text(value)
    ))
}

/// A submit button of the classes `class` showing `content`.
#[must_use]
pub fn submit(class: &str, content: &Html) -> Html {
    Html::raw(format!(
        "<button type=\"submit\" class=\"{}\">{content}</button>",
        Html::text(class)
    ))
}
