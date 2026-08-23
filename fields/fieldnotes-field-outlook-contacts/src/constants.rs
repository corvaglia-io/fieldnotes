//! Constants shared between the `describe` and `collect` operations.
//!
//! Keeping them in one place means the manifest this Field declares and the
//! records and cursors it actually emits can never drift apart by accident.

/// The registered A1 Field stem this Field's manifest declares.
pub(crate) const FIELD_STEM: &str = "outlook_contacts";

/// The registered connector property prefix, including its trailing
/// underscore.
pub(crate) const PROPERTY_PREFIX: &str = "outlook_contacts_";

/// The connector driver name.
pub(crate) const DRIVER_NAME: &str = "outlook-contacts-reference";

/// The Field's own cursor encoding version.
///
/// Changing this is a migration: core refuses to replay a stored cursor
/// written at a different version rather than handing this Field a token it
/// might misread (A2 section 4 and section 9).
pub(crate) const CURSOR_FORMAT_VERSION: u16 = 1;

/// The `contact` capability slice's connector-local object kind and the A1
/// primary Note type it maps to.
pub(crate) const OBJECT_KIND_CONTACT: &str = "contact";

/// The least-privilege Microsoft Graph scope this Field requests.
pub(crate) const GRAPH_SCOPE: &str = "Contacts.Read";

/// The non-secret configuration key naming the tenant a mailbox belongs to.
///
/// Required in this release: a contact's portable exact-source scope is the
/// tenant, matching the frozen `outlook_contacts_work` fixture's
/// `microsoft-graph:tenant/<tenant-id>` shape, and this Field does not yet
/// derive a tenant ID from the delegated access token itself (see the crate
/// documentation for why).
pub(crate) const CONFIG_TENANT_ID: &str = "tenant_id";

/// The non-secret configuration key naming a mailbox other than the signed-in
/// user's own.
///
/// Recognized but refused at configuration validation in this release: see
/// [`crate::config::ConfigError::MailboxUnsupported`] for why. Microsoft
/// Graph exposes a documented contacts-delta feed only under the signed-in
/// user's own contacts (`/me/contacts/delta`) or under a specific contact
/// folder (`/me/contactFolders/{id}/contacts/delta` or, for another mailbox,
/// `/users/{id}/contactFolders/{id}/contacts/delta`); there is no documented
/// path for another mailbox's contacts without naming a folder, and this
/// Field does not yet accept a contact-folder ID as configuration. Absent,
/// this Field collects `/me/contacts/delta`.
pub(crate) const CONFIG_MAILBOX: &str = "mailbox";

/// The non-secret configuration key overriding the Graph service root.
///
/// Real production use is a national-cloud endpoint
/// (`https://graph.microsoft.us`, for example); this Field's own tests point
/// it at a local loopback fixture server instead of a live tenant.
pub(crate) const CONFIG_GRAPH_BASE_URL: &str = "graph_base_url";

/// The anchor namespace an email address is emitted under.
pub(crate) const ANCHOR_NAMESPACE_EMAIL: &str = "email";

/// The anchor namespace a phone number is emitted under.
pub(crate) const ANCHOR_NAMESPACE_PHONE: &str = "phone";

/// The declared prefixed property naming a contact's employer.
pub(crate) const PROPERTY_COMPANY_NAME: &str = "outlook_contacts_company_name";

/// The declared prefixed property naming a contact's job title.
pub(crate) const PROPERTY_JOB_TITLE: &str = "outlook_contacts_job_title";

/// The declared prefixed property distinguishing a person from an
/// organization contact, per this Field's own source-observable heuristic
/// (see [`crate::record`]).
pub(crate) const PROPERTY_CONTACT_KIND: &str = "outlook_contacts_contact_kind";
