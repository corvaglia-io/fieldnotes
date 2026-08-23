//! Rendering one message's deterministic body evidence.
//!
//! A2 section 6 asks for "deterministically normalized source evidence as
//! Markdown text", and nothing more: core owns canonical bytes, the content
//! hash, and the filename. Two runs over the same message must produce the
//! same body text, so nothing here reads a clock, a locale, or a hash map
//! iteration order.
//!
//! # HTML bodies
//!
//! Almost every real mail body arrives as HTML. Converting HTML to faithful
//! Markdown is a large, ambiguous problem, and getting it subtly wrong changes
//! the content hash of every affected Note. This Field therefore does the
//! smaller, fully specified thing: it reduces HTML to plain text
//! deterministically -- `script` and `style` element content dropped, block
//! elements turned into line breaks, tags removed, character references
//! resolved -- and declares that reduction as a manifest limitation rather
//! than pretending to round-trip Markdown. The body is evidence, not an
//! archival copy; the message itself stays at its source, which `source_url`
//! names.

/// The block-level element names whose start or end tag becomes a line break.
const BLOCK_ELEMENTS: [&str; 17] = [
    "p",
    "div",
    "br",
    "tr",
    "li",
    "ul",
    "ol",
    "table",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "blockquote",
    "pre",
    "hr",
];

/// The elements whose *content* is markup rather than message text.
const DROPPED_ELEMENTS: [&str; 3] = ["script", "style", "head"];

/// Reduces an HTML mail body to deterministic plain text.
#[must_use]
pub(crate) fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    let mut dropping: Option<&'static str> = None;

    while !rest.is_empty() {
        match rest.find('<') {
            None => {
                if dropping.is_none() {
                    push_text(&mut out, rest);
                }
                break;
            }
            Some(index) => {
                if dropping.is_none() {
                    push_text(&mut out, &rest[..index]);
                }
                rest = &rest[index..];
                let Some(close) = rest.find('>') else {
                    // An unterminated tag: the remainder is markup, not text.
                    break;
                };
                let tag = &rest[1..close];
                rest = &rest[close + 1..];
                let (name, is_end) = tag_name(tag);
                match dropping {
                    Some(open) if is_end && open == name => dropping = None,
                    Some(_) => {}
                    None => {
                        if !is_end
                            && let Some(dropped) =
                                DROPPED_ELEMENTS.iter().find(|element| **element == name)
                        {
                            dropping = Some(dropped);
                        } else if BLOCK_ELEMENTS.contains(&name.as_str()) {
                            out.push('\n');
                        }
                    }
                }
            }
        }
    }
    normalize_lines(&decode_references(&out))
}

/// The lowercased element name of a tag body, and whether it was an end tag.
fn tag_name(tag: &str) -> (String, bool) {
    let trimmed = tag.trim();
    let (trimmed, is_end) = match trimmed.strip_prefix('/') {
        Some(rest) => (rest, true),
        None => (trimmed, false),
    };
    let name: String = trimmed
        .chars()
        .take_while(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    (name, is_end)
}

fn push_text(out: &mut String, text: &str) {
    out.push_str(text);
}

/// Resolves the character references a mail body actually uses, plus numeric
/// references. An unrecognized reference is left verbatim rather than guessed
/// at, so no text is invented.
fn decode_references(text: &str) -> String {
    const NAMED: [(&str, &str); 10] = [
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&apos;", "'"),
        ("&#39;", "'"),
        ("&nbsp;", " "),
        ("&hellip;", "\u{2026}"),
        ("&mdash;", "\u{2014}"),
        ("&ndash;", "\u{2013}"),
    ];
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    'outer: while let Some(index) = rest.find('&') {
        out.push_str(&rest[..index]);
        rest = &rest[index..];
        for (reference, replacement) in NAMED {
            if let Some(tail) = rest.strip_prefix(reference) {
                out.push_str(replacement);
                rest = tail;
                continue 'outer;
            }
        }
        if let Some((decoded, tail)) = numeric_reference(rest) {
            out.push(decoded);
            rest = tail;
            continue;
        }
        out.push('&');
        rest = &rest[1..];
    }
    out.push_str(rest);
    out
}

/// Decodes `&#NNN;` and `&#xHH;`, returning the character and the remaining
/// text.
fn numeric_reference(rest: &str) -> Option<(char, &str)> {
    let body = rest.strip_prefix("&#")?;
    let end = body.find(';')?;
    let digits = &body[..end];
    let (digits, radix) = match digits.strip_prefix(['x', 'X']) {
        Some(hex) => (hex, 16),
        None => (digits, 10),
    };
    if digits.is_empty() || digits.len() > 7 {
        return None;
    }
    let code = u32::from_str_radix(digits, radix).ok()?;
    let decoded = char::from_u32(code)?;
    Some((decoded, &body[end + 1..]))
}

/// Collapses runs of blank lines, trims trailing spaces, and normalizes line
/// endings, so the same message always renders the same text.
fn normalize_lines(text: &str) -> String {
    let unified = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = unified.split('\n').collect();
    let mut out = String::with_capacity(text.len());
    let mut blank_run = 0usize;
    let mut wrote_any = false;
    for line in lines {
        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() {
            blank_run += 1;
            continue;
        }
        if wrote_any {
            // At most one blank line survives between two non-blank lines.
            out.push('\n');
            if blank_run > 0 {
                out.push('\n');
            }
        }
        out.push_str(trimmed);
        wrote_any = true;
        blank_run = 0;
    }
    out
}

/// One line of attachment evidence for the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttachmentLine {
    /// The attachment's own filename, or its reference when it had none.
    pub(crate) label: String,
    /// The size the source declared, when it declared one.
    pub(crate) byte_length: Option<u64>,
    /// Whether the bytes were retained, and if not, why.
    pub(crate) note: String,
}

/// Assembles the record body: a heading, the message text, and deterministic
/// per-attachment evidence.
///
/// A1's flat-frontmatter rule leaves no room for per-attachment detail in
/// properties -- the property registry says so explicitly for
/// `skipped_attachments` -- so the reason one attachment was declined lives
/// here, in reviewable text, rather than as a stale copy of a policy decision
/// in frontmatter.
#[must_use]
pub(crate) fn render(subject: &str, text: &str, attachments: &[AttachmentLine]) -> String {
    let mut body = String::with_capacity(text.len() + 128);
    body.push_str("# ");
    body.push_str(subject);
    body.push_str("\n\n");
    if text.is_empty() {
        body.push_str("The source message carried no readable body content.\n");
    } else {
        body.push_str(text);
        body.push('\n');
    }
    if !attachments.is_empty() {
        body.push_str("\n## Attachments\n\n");
        for line in attachments {
            body.push_str("- `");
            body.push_str(&line.label);
            body.push('`');
            if let Some(bytes) = line.byte_length {
                body.push_str(&format!(" ({bytes} bytes)"));
            }
            body.push_str(": ");
            body.push_str(&line.note);
            body.push('\n');
        }
    }
    body
}

#[cfg(test)]
mod tests {
    use super::{AttachmentLine, html_to_text, render};

    #[test]
    fn a_plain_paragraph_becomes_plain_text() {
        assert_eq!(
            html_to_text("<html><body><p>Hi Sam,</p><p>Thursday works.</p></body></html>"),
            "Hi Sam,\n\nThursday works."
        );
    }

    #[test]
    fn a_line_break_becomes_a_line_break() {
        assert_eq!(
            html_to_text("Liebe Gr&uuml;sse<br>Alice M&#252;ller"),
            "Liebe Gr&uuml;sse\nAlice M\u{fc}ller",
            "an unrecognized named reference is left verbatim rather than guessed at"
        );
    }

    #[test]
    fn script_and_style_content_never_reaches_the_body() {
        let html = "<style>p{color:red}</style><p>Visible</p><script>alert('x')</script>";
        assert_eq!(html_to_text(html), "Visible");
    }

    #[test]
    fn character_references_resolve() {
        assert_eq!(
            html_to_text("<p>5 &lt; 6 &amp;&amp; 7 &gt; 6 &nbsp;&#x2014;</p>"),
            "5 < 6 && 7 > 6  \u{2014}"
        );
    }

    #[test]
    fn the_reduction_is_deterministic() {
        let html = "<div><p>One</p>\r\n\r\n<p>Two</p></div>";
        assert_eq!(html_to_text(html), html_to_text(html));
        assert_eq!(html_to_text(html), "One\n\nTwo");
    }

    #[test]
    fn an_unterminated_tag_does_not_leak_markup_or_panic() {
        assert_eq!(html_to_text("<p>Visible</p><img src=\"x"), "Visible");
    }

    #[test]
    fn multibyte_content_survives_intact() {
        assert_eq!(
            html_to_text("<p>caf\u{e9} \u{65e5}\u{672c}\u{8a9e}</p>"),
            "caf\u{e9} \u{65e5}\u{672c}\u{8a9e}"
        );
    }

    #[test]
    fn the_body_carries_the_subject_as_its_heading() {
        let body = render("Migration Thursday", "Hi Sam,", &[]);
        assert!(body.starts_with("# Migration Thursday\n\nHi Sam,"));
    }

    #[test]
    fn an_empty_body_says_so_rather_than_being_empty() {
        let body = render("No text", "", &[]);
        assert!(body.contains("carried no readable body content"));
    }

    #[test]
    fn attachment_evidence_records_the_decline_reason_in_the_body() {
        let body = render(
            "Team standup recording",
            "Sharing this week's notes.",
            &[
                AttachmentLine {
                    label: "notes.txt".to_owned(),
                    byte_length: Some(40),
                    note: "retained as an original artifact".to_owned(),
                },
                AttachmentLine {
                    label: "team-standup-recording.mp4".to_owned(),
                    byte_length: Some(641_728_512),
                    note: "not retained: media type video/mp4 is outside this run's retention \
                           include set, so it stays at its source"
                        .to_owned(),
                },
            ],
        );
        assert!(body.contains("## Attachments"));
        assert!(body.contains("`notes.txt` (40 bytes): retained"));
        assert!(body.contains("video/mp4"));
    }
}
