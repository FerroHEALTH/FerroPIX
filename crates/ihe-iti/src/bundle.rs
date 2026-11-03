// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! What the FHIR clients read from a `Bundle` alike: an entry's `fullUrl`,
//! the logical id a request URL names, and the `next` link.

#[cfg(any(feature = "mcsd", feature = "pmir"))]
use fhir_types::r4::bundle::BundleEntry;
#[cfg(any(feature = "mcsd", feature = "pdqm"))]
use fhir_types::r4::bundle::BundleLink;
#[cfg(any(feature = "mcsd", feature = "pdqm"))]
use url::Url;

/// The entry's `fullUrl`, when it carries a non-empty one.
#[cfg(any(feature = "mcsd", feature = "pmir"))]
pub(crate) fn full_url(entry: &BundleEntry) -> Option<String> {
    entry
        .full_url
        .as_ref()
        .and_then(|full_url| full_url.value.clone())
        .filter(|full_url| !full_url.is_empty())
}

/// The logical id `url` names for `resource_type`: `[type]/[id]` or
/// `[type]/[id]/_history/[vid]`, relative or under any base (FHIR R4 http,
/// <http://hl7.org/fhir/R4/http.html#general>).
#[cfg(any(feature = "mcsd", feature = "pmir"))]
pub(crate) fn logical_id(url: &str, resource_type: &str) -> Option<String> {
    let path = url.split(['?', '#']).next()?;
    let segments: Vec<&str> = path.split('/').collect();
    let at = segments
        .iter()
        .rposition(|segment| *segment == resource_type)?;
    match segments.get(at.saturating_add(1)..)? {
        [id] | [id, "_history", _] if !id.is_empty() => Some((*id).to_owned()),
        _ => None,
    }
}

/// A `next` link with no URL, or one that does not resolve.
#[cfg(any(feature = "mcsd", feature = "pdqm"))]
#[derive(Debug)]
pub(crate) struct BadNext;

/// The `next` link of `links`, resolved against the FHIR base (FHIR R4
/// paging, <http://hl7.org/fhir/R4/http.html#paging>).
#[cfg(any(feature = "mcsd", feature = "pdqm"))]
pub(crate) fn next(links: &[BundleLink], base: &Url) -> Result<Option<Url>, BadNext> {
    let Some(link) = links
        .iter()
        .find(|link| link.relation.value.as_deref() == Some("next"))
    else {
        return Ok(None);
    };
    let url = link.url.value.as_deref().ok_or(BadNext)?;
    base.join(url).map(Some).map_err(|_unparsable| BadNext)
}
