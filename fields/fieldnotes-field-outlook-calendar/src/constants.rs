//! Constants shared between the `describe` and `collect` operations.
//!
//! Keeping them in one place means the manifest this Field declares and the
//! records and cursors it actually emits can never drift apart by accident.

/// The registered A1 Field stem this Field's manifest declares.
pub(crate) const FIELD_STEM: &str = "outlook_calendar";

/// The registered connector property prefix, including its trailing
/// underscore.
pub(crate) const PROPERTY_PREFIX: &str = "outlook_calendar_";

/// The connector driver name.
pub(crate) const DRIVER_NAME: &str = "outlook-calendar";

/// The Field's own cursor encoding version.
///
/// Changing this is a migration: core refuses to replay a stored cursor
/// written at a different version rather than handing this Field a token it
/// might misread (A2 section 4 and section 9).
pub(crate) const CURSOR_FORMAT_VERSION: u16 = 1;

/// The one capability slice this release supports: a calendar event
/// (single-instance, occurrence, or exception -- never a bare series master,
/// since this Field only ever collects through Graph's `calendarView`, which
/// expands a recurring series into its instances).
pub(crate) const OBJECT_KIND_EVENT: &str = "calendar-event";

/// The A1 primary Note type the `calendar-event` object kind maps to.
pub(crate) const NOTE_TYPE_EVENT: &str = "event";

/// The non-secret configuration key naming the Entra tenant this Field
/// collects against. Combined with [`SCOPE_PREFIX`] to derive the portable
/// exact-source scope.
pub(crate) const CONFIG_TENANT_ID: &str = "tenant_id";

/// The portable exact-source scope prefix, matching every other Microsoft
/// Field's `microsoft-graph:tenant/<tenant-guid>` shape.
pub(crate) const SCOPE_PREFIX: &str = "microsoft-graph:tenant/";

/// The object-kind namespace a source identity is built under:
/// `calendar-event/<graph-event-id>`.
pub(crate) const IDENTITY_NAMESPACE: &str = "calendar-event";

/// The object-kind namespace an occurrence's or exception's `parent_identity`
/// is built under: `calendar-event-series/<graph-series-master-id>`. Kept
/// distinct from [`IDENTITY_NAMESPACE`] so a series reference can never be
/// mistaken for an ordinary event's own portable identity.
pub(crate) const SERIES_IDENTITY_NAMESPACE: &str = "calendar-event-series";

/// The least-privilege Graph scope this Field requests.
pub(crate) const GRAPH_SCOPE: &str = "Calendars.Read";

/// The Graph fields this Field selects on the initial `calendarView/delta`
/// request, joined with `,` before percent-encoding. `id`, `subject`,
/// `start`, `end`, `isAllDay`, `organizer`, and `attendees` map onto A1
/// vocabulary; `isCancelled`, `type`, and `seriesMasterId` back this Field's
/// own prefixed properties and its recurrence handling; `webLink`,
/// `changeKey`, and `responseStatus` are display and versioning evidence
/// only. A resumed request built from a stored `@odata.deltaLink` carries no
/// `$select` of its own -- Graph already fixed the projection when the link
/// was minted -- so this list is read only when starting a fresh delta.
pub(crate) const SELECT_FIELDS: [&str; 13] = [
    "id",
    "subject",
    "start",
    "end",
    "isAllDay",
    "isCancelled",
    "organizer",
    "attendees",
    "type",
    "seriesMasterId",
    "webLink",
    "changeKey",
    "responseStatus",
];

/// The environment variable, read only by this Field's `main`, that points a
/// child-process test at a sanitized recorded response script instead of the
/// real Microsoft Graph endpoint. Unset in production. Mirrors the
/// `FIELDNOTES_FIXTURE_EXIT_CODE`-style override the fixture Field already
/// uses for the same reason: a deterministic, network-free, real-child-process
/// test seam that lives entirely in the composition root.
pub(crate) const FIXTURE_SCRIPT_ENV: &str = "FIELDNOTES_OUTLOOK_CALENDAR_FIXTURE_SCRIPT";
