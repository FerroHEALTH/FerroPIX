// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! What a page is drawn from: the [`PageContext`] the server read for the
//! request, the locale it names and the route's parameters.
//!
//! Every page is drawn once, whole, from one [`Visit`], so the words, the
//! preferences and the data of a page all come from the same request. No
//! specification governs the console's request model: our own design.

use ferrohealth_ui::i18n::{Arg, Locale};
use ferrohealth_ui::theme::Preferences;

use crate::locale::messages::Msg;
use crate::locale::{tr, tr_with};
use crate::model::PageContext;

/// One request, as a page reads it.
#[derive(Debug, Clone)]
pub struct Visit {
    /// The locale the page is drawn in.
    pub locale: Locale,
    /// What the server read for the page.
    pub page: PageContext,
    /// The route parameters of the page, by name.
    pub params: Vec<(String, String)>,
}

impl Visit {
    /// The visit of the page `page` is the context of, with no route
    /// parameters.
    #[must_use]
    pub fn new(page: PageContext) -> Self {
        Self {
            locale: locale_of(&page),
            page,
            params: Vec::new(),
        }
    }

    /// This visit with the route parameters `params`.
    #[must_use]
    pub fn with_params(self, params: Vec<(String, String)>) -> Self {
        Self { params, ..self }
    }

    /// The console's message `msg` in the visit's locale.
    #[must_use]
    pub fn tr(&self, msg: Msg) -> String {
        tr(&self.locale, msg)
    }

    /// The console's message `msg` in the visit's locale, with the
    /// placeables `args` filled in.
    #[must_use]
    pub fn tr_with(&self, msg: Msg, args: &[(&'static str, Arg)]) -> String {
        tr_with(&self.locale, msg, args)
    }

    /// The route parameter `name`.
    #[must_use]
    pub fn param(&self, name: &str) -> Option<String> {
        self.params
            .iter()
            .find(|(each, _)| each == name)
            .map(|(_, value)| value.clone())
    }

    /// The path of the page, without its query.
    #[must_use]
    pub fn path(&self) -> &str {
        self.page.path.split(['?', '#']).next().unwrap_or_default()
    }

    /// The query of the page, without its `?`, empty for none.
    #[must_use]
    pub fn query(&self) -> &str {
        self.page
            .path
            .split_once('?')
            .map(|(_, query)| query.split('#').next().unwrap_or_default())
            .unwrap_or_default()
    }

    /// The query word `name` of the page.
    #[must_use]
    pub fn query_word(&self, name: &str) -> Option<&str> {
        self.query()
            .split('&')
            .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
    }

    /// The steward's preferences the page's cookies keep.
    #[must_use]
    pub fn preferences(&self) -> Preferences {
        Preferences::read(&self.page.preferences, &self.page.prefix)
    }
}

/// The locale of the page `page`, English for a tag no catalog names.
#[must_use]
pub fn locale_of(page: &PageContext) -> Locale {
    if page.locale == crate::locale::PSEUDO_TAG {
        return crate::locale::pseudo();
    }
    crate::locale::choices()
        .into_iter()
        .find(|locale| locale.tag() == page.locale)
        .unwrap_or_else(Locale::english)
}

#[cfg(test)]
mod tests {
    use super::Visit;
    use crate::model::PageContext;

    #[test]
    fn a_visit_reads_its_path_query_and_locale() {
        let visit = Visit::new(PageContext {
            path: String::from("/patients/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b?tab=history"),
            locale: String::from("qps"),
            ..PageContext::signed_out(String::new())
        })
        .with_params(vec![(String::from("person"), String::from("p"))]);
        assert_eq!(
            "/patients/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
            visit.path()
        );
        assert_eq!("tab=history", visit.query());
        assert_eq!(Some("history"), visit.query_word("tab"));
        assert_eq!(None, visit.query_word("ta"));
        assert_eq!(Some(String::from("p")), visit.param("person"));
        assert_eq!("qps", visit.locale.tag());
    }
}
