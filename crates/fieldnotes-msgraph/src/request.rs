//! A read-only Microsoft Graph request builder.
//!
//! [`GraphRequest`] can only ever describe a `GET`. There is no method
//! field, no way to attach a body, and no builder step that would let a
//! caller construct a `POST`, `PATCH`, `PUT`, or `DELETE`: collection is
//! read-only across the whole product, and this type makes a write
//! structurally impossible rather than merely undocumented.

use std::fmt;

use crate::url::percent_encode_query_value;

/// A `$select`/`$top`/`$filter`/`$search`/`$orderby`-qualified read against
/// one Graph resource.
///
/// Construct with [`GraphRequest::new`] and narrow it with the builder
/// methods. [`GraphRequest::filter`] and [`GraphRequest::search`] refuse to
/// be combined on the same request, matching the Graph constraint that
/// `$filter` and `$search` are mutually exclusive; [`GraphRequest::search`]
/// double-quotes its value automatically, matching the separate Graph
/// constraint that a `$search` value must be quoted.
#[derive(Debug, Clone)]
pub struct GraphRequest {
    resource: String,
    select: Vec<String>,
    filter: Option<String>,
    search: Option<String>,
    order_by: Option<String>,
    top: Option<u32>,
}

/// A request could not be built as specified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestBuildError {
    /// `$filter` and `$search` were both requested on the same
    /// [`GraphRequest`]. Graph rejects this combination outright, so this
    /// crate refuses to send it.
    FilterAndSearchCombined,
}

impl fmt::Display for RequestBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RequestBuildError::FilterAndSearchCombined => f.write_str(
                "a Graph request cannot combine $filter and $search; Graph rejects the combination",
            ),
        }
    }
}

impl std::error::Error for RequestBuildError {}

impl GraphRequest {
    /// Starts a request against `resource`, a Graph-relative path such as
    /// `/me/messages` or `/me/mailFolders('Inbox')/messages`.
    ///
    /// A missing leading slash is added; this builder never validates that
    /// `resource` is a real Graph path, since that is a per-connector
    /// concern this crate does not own.
    #[must_use]
    pub fn new(resource: impl Into<String>) -> Self {
        let resource = resource.into();
        let resource = if resource.starts_with('/') {
            resource
        } else {
            format!("/{resource}")
        };
        GraphRequest {
            resource,
            select: Vec::new(),
            filter: None,
            search: None,
            order_by: None,
            top: None,
        }
    }

    /// Restricts the response to exactly the named fields, via `$select`.
    ///
    /// Requesting only the fields a connector maps keeps both the response
    /// this crate has to bound and parse, and the surface a downstream
    /// mapping has to consider, as small as the connector allows.
    #[must_use]
    pub fn select<I, S>(mut self, fields: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.select = fields.into_iter().map(Into::into).collect();
        self
    }

    /// Applies a `$filter` predicate.
    ///
    /// Fails with [`RequestBuildError::FilterAndSearchCombined`] if
    /// [`GraphRequest::search`] was already applied.
    pub fn filter(mut self, expression: impl Into<String>) -> Result<Self, RequestBuildError> {
        if self.search.is_some() {
            return Err(RequestBuildError::FilterAndSearchCombined);
        }
        self.filter = Some(expression.into());
        Ok(self)
    }

    /// Applies a `$search` phrase.
    ///
    /// The phrase is double-quoted automatically, escaping any embedded
    /// double quote, because Graph requires every `$search` value to be
    /// quoted. Fails with [`RequestBuildError::FilterAndSearchCombined`] if
    /// [`GraphRequest::filter`] was already applied.
    pub fn search(mut self, phrase: impl AsRef<str>) -> Result<Self, RequestBuildError> {
        if self.filter.is_some() {
            return Err(RequestBuildError::FilterAndSearchCombined);
        }
        let escaped = phrase.as_ref().replace('"', "\\\"");
        self.search = Some(format!("\"{escaped}\""));
        Ok(self)
    }

    /// Applies an `$orderby` expression.
    #[must_use]
    pub fn order_by(mut self, expression: impl Into<String>) -> Self {
        self.order_by = Some(expression.into());
        self
    }

    /// Bounds the page size via `$top`.
    #[must_use]
    pub fn top(mut self, top: u32) -> Self {
        self.top = Some(top);
        self
    }

    /// Rewrites this request to target the resource's delta collection,
    /// appending `/delta` to the resource path and carrying every other
    /// parameter unchanged.
    #[must_use]
    pub(crate) fn into_delta(mut self) -> Self {
        self.resource = format!("{}/delta", self.resource);
        self
    }

    fn query_string(&self) -> String {
        let mut pairs: Vec<(&'static str, String)> = Vec::new();
        if !self.select.is_empty() {
            pairs.push(("$select", self.select.join(",")));
        }
        if let Some(filter) = &self.filter {
            pairs.push(("$filter", filter.clone()));
        }
        if let Some(search) = &self.search {
            pairs.push(("$search", search.clone()));
        }
        if let Some(order_by) = &self.order_by {
            pairs.push(("$orderby", order_by.clone()));
        }
        if let Some(top) = self.top {
            pairs.push(("$top", top.to_string()));
        }
        pairs
            .into_iter()
            .map(|(key, value)| format!("{key}={}", percent_encode_query_value(&value)))
            .collect::<Vec<_>>()
            .join("&")
    }

    /// Builds the absolute URL this request describes, against `base_url`.
    pub(crate) fn into_url(self, base_url: &str) -> String {
        let mut url = format!("{}{}", base_url.trim_end_matches('/'), self.resource);
        let query = self.query_string();
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query);
        }
        url
    }
}

#[cfg(test)]
mod tests {
    use super::{GraphRequest, RequestBuildError};

    const BASE: &str = "https://graph.microsoft.com/v1.0";

    /// Unwraps a build result in a test, without `unwrap`/`expect` (denied
    /// workspace-wide, including in tests).
    fn must_build(result: Result<GraphRequest, RequestBuildError>) -> GraphRequest {
        match result {
            Ok(request) => request,
            Err(error) => panic!("expected a valid request, got {error}"),
        }
    }

    #[test]
    fn a_bare_resource_needs_no_query_string() {
        let url = GraphRequest::new("/me/messages").into_url(BASE);
        assert_eq!(url, "https://graph.microsoft.com/v1.0/me/messages");
    }

    #[test]
    fn a_missing_leading_slash_is_added() {
        let url = GraphRequest::new("me/messages").into_url(BASE);
        assert_eq!(url, "https://graph.microsoft.com/v1.0/me/messages");
    }

    #[test]
    fn select_and_top_are_rendered_as_query_parameters() {
        let url = GraphRequest::new("/me/messages")
            .select(["subject", "from"])
            .top(25)
            .into_url(BASE);
        assert_eq!(
            url,
            "https://graph.microsoft.com/v1.0/me/messages?$select=subject%2Cfrom&$top=25"
        );
    }

    #[test]
    fn search_values_are_double_quoted_automatically() {
        let url =
            must_build(GraphRequest::new("/me/messages").search("quarterly report")).into_url(BASE);
        assert_eq!(
            url,
            "https://graph.microsoft.com/v1.0/me/messages?$search=%22quarterly%20report%22"
        );
    }

    #[test]
    fn filter_and_search_cannot_be_combined() {
        let with_filter =
            must_build(GraphRequest::new("/me/messages").filter("importance eq 'high'"));
        match with_filter.search("quarterly") {
            Err(error) => assert_eq!(error, RequestBuildError::FilterAndSearchCombined),
            Ok(_) => panic!("combining filter and search should have been refused"),
        }

        let with_search = must_build(GraphRequest::new("/me/messages").search("quarterly"));
        match with_search.filter("importance eq 'high'") {
            Err(error) => assert_eq!(error, RequestBuildError::FilterAndSearchCombined),
            Ok(_) => panic!("combining filter and search should have been refused"),
        }
    }

    #[test]
    fn delta_appends_the_delta_segment_and_keeps_other_parameters() {
        let url = GraphRequest::new("/me/mailFolders('Inbox')/messages")
            .select(["subject"])
            .into_delta()
            .into_url(BASE);
        assert_eq!(
            url,
            "https://graph.microsoft.com/v1.0/me/mailFolders('Inbox')/messages/delta?$select=subject"
        );
    }
}
