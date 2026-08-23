//! Fetching a contact's photo bytes.
//!
//! # Why this exists outside `fieldnotes-msgraph`
//!
//! A Graph `contact` resource carries no field saying whether a photo exists
//! or how large one is -- unlike `user`, there is no cheap metadata check.
//! The only way to know is to call the binary `/photo/$value` endpoint and
//! see whether it answers `200` or `404`. `fieldnotes_msgraph::GraphClient`
//! is deliberately JSON-only (`GraphClient::get`/`list`/`delta` all decode a
//! `DeserializeOwned` response), and the pieces that would be needed to do a
//! raw byte fetch through it -- `GraphHttpRequest::new` and
//! `AccessToken::as_str`/`header_value` -- are `pub(crate)` to that crate. So
//! this module depends on `ureq` directly, with the same TLS backend and
//! feature set `fieldnotes-msgraph` already uses, and implements exactly the
//! one call it needs. See this crate's final report for the
//! `fieldnotes-msgraph::GraphClient::get_bytes`-shaped gap this stands in
//! for.
//!
//! [`PhotoTransport`] is the seam that keeps this network dependency out of
//! this Field's own tests: [`UreqPhotoTransport`] is the shipping
//! implementation, and a test substitutes a fake that answers from memory,
//! exactly mirroring `fieldnotes_msgraph::testing::ScriptedTransport`.

use std::fmt;
use std::time::Duration;

/// One fetched photo: its bytes and the media type Graph declared for them.
pub(crate) struct FetchedPhoto {
    pub(crate) bytes: Vec<u8>,
    /// Graph's `Content-Type` response header, when it sent one. Contact
    /// photos are conventionally JPEG, but this Field never assumes that:
    /// an absent header means an undeclared media type, exactly as
    /// `fields/fieldnotes-field-local` never guesses one from a filename.
    pub(crate) media_type: Option<String>,
}

/// Why fetching a photo failed outright (as opposed to the contact simply
/// having none, which is [`Option::None`], not an error).
#[derive(Debug)]
pub(crate) struct PhotoFetchError(pub(crate) String);

impl fmt::Display for PhotoFetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PhotoFetchError {}

/// The seam between this module's retention-policy mapping and the transport
/// that actually reaches Graph (or, in a test, does not).
pub(crate) trait PhotoTransport {
    /// Fetches the photo at `url`, authenticated with `bearer_token`.
    ///
    /// `Ok(None)` means the contact has no photo (Graph answered `404`).
    /// `Err` means the fetch could not be trusted at all: this Field treats
    /// that as "no photo obtained this run" and reports it as a diagnostic
    /// rather than failing the whole record, since one contact's photo
    /// endpoint misbehaving must not cost the rest of the run.
    fn fetch(&self, url: &str, bearer_token: &str)
    -> Result<Option<FetchedPhoto>, PhotoFetchError>;
}

/// The shipping [`PhotoTransport`]: a single blocking `ureq` `GET`.
pub(crate) struct UreqPhotoTransport {
    agent: ureq::Agent,
}

impl UreqPhotoTransport {
    /// Builds a transport with a bounded per-call timeout and an HTTPS-only,
    /// no-cross-authority-redirect agent, matching
    /// `fieldnotes_msgraph::transport::UreqTransport`'s own configuration.
    pub(crate) fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .https_only(true)
            .timeout_per_call(Some(Duration::from_secs(30)))
            .build();
        UreqPhotoTransport {
            agent: config.into(),
        }
    }
}

/// A conservative cap on a fetched photo's bytes, applied here as defense in
/// depth before this module's own retention-size comparison ever runs. Well
/// above the frozen 512 MiB single-artifact ceiling is pointless to allow: no
/// contact photo is anywhere near that large, and refusing early avoids
/// buffering a hostile or corrupted response fully into memory.
const MAX_PHOTO_RESPONSE_BYTES: u64 = 64 * 1024 * 1024;

impl PhotoTransport for UreqPhotoTransport {
    fn fetch(
        &self,
        url: &str,
        bearer_token: &str,
    ) -> Result<Option<FetchedPhoto>, PhotoFetchError> {
        let mut response = self
            .agent
            .get(url)
            .header("Authorization", format!("Bearer {bearer_token}"))
            .call()
            .map_err(|error| PhotoFetchError(error.to_string()))?;
        if response.status() == 404 {
            return Ok(None);
        }
        if !(200..300).contains(&response.status().as_u16()) {
            return Err(PhotoFetchError(format!(
                "the photo endpoint answered status {}",
                response.status()
            )));
        }
        let media_type = response
            .headers()
            .get("Content-Type")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let bytes = response
            .body_mut()
            .with_config()
            .limit(MAX_PHOTO_RESPONSE_BYTES)
            .read_to_vec()
            .map_err(|error| PhotoFetchError(error.to_string()))?;
        Ok(Some(FetchedPhoto { bytes, media_type }))
    }
}

#[cfg(test)]
pub(crate) mod testing {
    //! An in-memory [`PhotoTransport`] double, so this Field's own tests never
    //! touch the network -- mirroring
    //! `fieldnotes_msgraph::testing::ScriptedTransport`.

    use super::{FetchedPhoto, PhotoFetchError, PhotoTransport};
    use std::cell::RefCell;
    use std::collections::VecDeque;

    /// What one scripted fetch answers.
    pub(crate) enum Scripted {
        /// The contact has a photo.
        Photo {
            bytes: Vec<u8>,
            media_type: Option<String>,
        },
        /// The contact has no photo (a `404`).
        None,
        /// The fetch fails outright.
        Err(String),
    }

    /// A [`PhotoTransport`] that answers a fixed, ordered script, one call per
    /// entry, and records every URL requested.
    pub(crate) struct ScriptedPhotoTransport {
        script: RefCell<VecDeque<Scripted>>,
        requested: RefCell<Vec<String>>,
    }

    impl ScriptedPhotoTransport {
        pub(crate) fn new(script: Vec<Scripted>) -> Self {
            ScriptedPhotoTransport {
                script: RefCell::new(script.into_iter().collect()),
                requested: RefCell::new(Vec::new()),
            }
        }

        pub(crate) fn requested_urls(&self) -> Vec<String> {
            self.requested.borrow().clone()
        }
    }

    impl PhotoTransport for ScriptedPhotoTransport {
        fn fetch(
            &self,
            url: &str,
            _bearer_token: &str,
        ) -> Result<Option<FetchedPhoto>, PhotoFetchError> {
            self.requested.borrow_mut().push(url.to_owned());
            match self.script.borrow_mut().pop_front() {
                Some(Scripted::Photo { bytes, media_type }) => {
                    Ok(Some(FetchedPhoto { bytes, media_type }))
                }
                Some(Scripted::None) => Ok(None),
                Some(Scripted::Err(message)) => Err(PhotoFetchError(message)),
                None => Err(PhotoFetchError(
                    "scripted photo transport exhausted".to_owned(),
                )),
            }
        }
    }
}
