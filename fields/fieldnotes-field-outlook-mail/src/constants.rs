//! Constants shared between the `describe` and `collect` operations.
//!
//! Keeping them in one place means the manifest this Field declares and the
//! records, cursors, and scopes it actually emits can never drift apart by
//! accident.

/// The registered A1 Field stem this Field's manifest declares.
pub(crate) const FIELD_STEM: &str = "outlook_mail";

/// The registered connector property prefix, including its trailing
/// underscore.
pub(crate) const PROPERTY_PREFIX: &str = "outlook_mail_";

/// The connector driver name.
pub(crate) const DRIVER_NAME: &str = "outlook-mail-graph";

/// The Field's own cursor encoding version.
///
/// Changing this is a migration: core refuses to replay a stored cursor
/// written at a different version rather than handing this Field a token it
/// might misread (A2 section 4 and section 9).
pub(crate) const CURSOR_FORMAT_VERSION: u16 = 1;

/// The single capability slice's connector-local object kind, which is also
/// the object-kind namespace inside every source identity this Field emits.
pub(crate) const OBJECT_KIND_MAIL_MESSAGE: &str = "mail-message";

/// The A1 primary Note type the `mail-message` slice maps onto.
pub(crate) const NOTE_TYPE_MAIL: &str = "mail";

/// The object-kind namespace an upstream attachment reference carries, per
/// the frozen `outlook_mail_work` fixtures' `skipped_attachments` entries.
pub(crate) const ATTACHMENT_KIND: &str = "mail-attachment";

/// The namespace prefix the portable exact-source scope is built from.
pub(crate) const SCOPE_NAMESPACE: &str = "microsoft-graph:tenant";

/// The namespace prefix the shared `thread_id` property carries, per the
/// frozen fixtures.
pub(crate) const THREAD_NAMESPACE: &str = "outlook-thread";

/// The least-privilege, read-only Graph scope this Field needs.
pub(crate) const GRAPH_SCOPE: &str = "Mail.Read";

/// The non-secret configuration key naming the Microsoft Entra tenant this
/// mailbox belongs to.
pub(crate) const CONFIG_TENANT_ID: &str = "tenant_id";

/// The non-secret configuration key naming the mail folder to collect from.
pub(crate) const CONFIG_MAIL_FOLDER: &str = "mail_folder";

/// The mail folder collected when `config.mail_folder` is absent.
pub(crate) const DEFAULT_MAIL_FOLDER: &str = "inbox";

/// The environment variable that puts this Field into fixture-replay mode.
///
/// It names a **script of sanitized recorded Graph responses**, never a
/// secret: fixture mode makes no network call and holds no token at all. It
/// exists so this Field's own conformance cases can spawn the real binary as a
/// real child process without a tenant, a network, or credentials, exactly as
/// `AGENTS.md` requires ("prefer recorded, sanitized fixtures over
/// live-account dependencies").
pub(crate) const FIXTURE_SCRIPT_VARIABLE: &str = "FIELDNOTES_OUTLOOK_MAIL_FIXTURE_SCRIPT";

/// The page size this Field requests from Graph.
///
/// Graph caps mail pages well below this in practice; asking for a bounded
/// page keeps one page's worth of messages resident at a time rather than
/// however many the service would otherwise choose.
pub(crate) const PAGE_SIZE: u32 = 50;
