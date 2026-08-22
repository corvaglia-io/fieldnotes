//! The closed vocabulary of the eleven approved primary Note types.

use core::fmt;

/// One of the eleven approved v0.1 primary Note types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NoteType {
    /// User-authored general Note.
    Text,
    /// Chat or message-system item.
    Message,
    /// Mail message where distinct mail semantics are useful.
    Mail,
    /// Meeting record where the meeting itself is primary.
    Meeting,
    /// Observed call record without an imported recording as its identity.
    Call,
    /// Issue or ticket-system item.
    Ticket,
    /// Source document whose text-bearing document identity is primary.
    Document,
    /// Generic imported or collected file.
    File,
    /// Source contact record.
    Contact,
    /// Calendar or other time-bounded event.
    Event,
    /// User-supplied playable voice recording.
    Voice,
}

impl NoteType {
    /// Every approved primary Note type.
    pub const ALL: [NoteType; 11] = [
        NoteType::Text,
        NoteType::Message,
        NoteType::Mail,
        NoteType::Meeting,
        NoteType::Call,
        NoteType::Ticket,
        NoteType::Document,
        NoteType::File,
        NoteType::Contact,
        NoteType::Event,
        NoteType::Voice,
    ];

    /// The approved lowercase spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            NoteType::Text => "text",
            NoteType::Message => "message",
            NoteType::Mail => "mail",
            NoteType::Meeting => "meeting",
            NoteType::Call => "call",
            NoteType::Ticket => "ticket",
            NoteType::Document => "document",
            NoteType::File => "file",
            NoteType::Contact => "contact",
            NoteType::Event => "event",
            NoteType::Voice => "voice",
        }
    }

    /// Parses an approved primary Note type; anything else is `None`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        NoteType::ALL.into_iter().find(|t| t.as_str() == text)
    }
}

impl fmt::Display for NoteType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vocabulary_is_closed() {
        assert_eq!(NoteType::parse("mail"), Some(NoteType::Mail));
        assert_eq!(NoteType::parse("voice"), Some(NoteType::Voice));
        assert_eq!(NoteType::parse("email"), None);
        assert_eq!(NoteType::parse("Mail"), None);
    }
}
