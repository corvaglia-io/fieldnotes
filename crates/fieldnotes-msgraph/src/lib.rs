//! Shared Microsoft Graph transport for first-party Microsoft Fields.
//!
//! Outlook Mail (`0.1.3`), then Outlook Calendar and Contacts (`0.1.4`), and
//! Microsoft Teams (`0.1.5`) all authenticate and read through this crate.
//! Per [A0](../../../docs/approvals/A0-repository-scaffold.md), the Microsoft
//! Fields share transport and authentication *code* here without sharing
//! user-facing property prefixes and without this crate importing a host
//! credential-store adapter — so this crate is transport, not connector
//! logic, and not credential storage:
//!
//! - it never acquires, refreshes, or stores an access token — a caller
//!   supplies one, as an opaque [`AccessToken`], for exactly the requests it
//!   authorizes;
//! - it never decides what a mail message, event, contact, or chat message
//!   looks like — [`GraphClient::get`], [`GraphClient::list`], and
//!   [`GraphClient::delta`] are generic over the response type a connector
//!   deserializes into;
//! - it exposes no way to construct anything but a `GET` — see
//!   [`request::GraphRequest`] and [`transport::GraphHttpRequest`] — because
//!   collection is read-only across the whole product and a write should be
//!   structurally impossible, not merely undocumented.
//!
//! # Shape of a call
//!
//! ```no_run
//! use fieldnotes_msgraph::{
//!     AccessToken, GraphClient, GraphRequest, SystemRetryClock, UreqTransport,
//! };
//!
//! # struct RealRandomSource;
//! # impl fieldnotes_msgraph::RandomSource for RealRandomSource {
//! #     fn fill_bytes(&mut self, buffer: &mut [u8]) { buffer.fill(0); }
//! # }
//! # fn example(access_token: AccessToken, real_random: RealRandomSource) -> Result<(), Box<dyn std::error::Error>> {
//! let client = GraphClient::new(UreqTransport::new(), SystemRetryClock::new(), real_random);
//!
//! #[derive(serde::Deserialize)]
//! struct MessageSummary {
//!     subject: Option<String>,
//! }
//!
//! let request = GraphRequest::new("/me/messages").select(["subject"]).top(25);
//! for item in client.list::<MessageSummary>(&access_token, request, "list mail messages") {
//!     let message = item?;
//!     let _ = message.subject;
//! }
//! # Ok(())
//! # }
//! ```
//!
//! The `RealRandomSource` above stands in for whatever random-byte source
//! the composing binary supplies for retry jitter. This crate deliberately
//! ships no such source itself, only the trait — see the module docs on
//! [`clock::SystemRetryClock`] for why that adapter is shipped here while a
//! production randomness source is not.

pub mod client;
pub mod clock;
pub mod error;
pub mod page;
pub mod request;
pub mod testing;
pub mod token;
pub mod transport;
mod url;

pub use client::{DEFAULT_BASE_URL, GraphClient, RetryPolicy};
pub use clock::{RetryClock, SystemRetryClock};
pub use error::{GraphError, GraphErrorDetail};
pub use fieldnotes_domain::RandomSource;
pub use page::{DeltaStart, DeltaToken, PageStream};
pub use request::{GraphRequest, RequestBuildError};
pub use token::AccessToken;
pub use transport::{
    GraphHttpRequest, GraphHttpResponse, HttpTransport, TransportError, UreqTransport,
};
