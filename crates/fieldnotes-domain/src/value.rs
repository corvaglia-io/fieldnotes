//! The flat scalar value model shared by frontmatter and record encodings.
//!
//! Allowed values are text, finite IEEE 754 binary64 numbers, booleans,
//! `YYYY-MM-DD` dates, explicit-offset datetimes, and homogeneous
//! one-dimensional lists of one scalar type.

use crate::datetime::{Date, Datetime};

/// The type of a scalar frontmatter value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScalarKind {
    /// A text value.
    Text,
    /// A finite binary64 number.
    Number,
    /// A `true`/`false` boolean.
    Bool,
    /// A `YYYY-MM-DD` calendar date.
    Date,
    /// An explicit-offset RFC 3339 datetime.
    Datetime,
}

impl ScalarKind {
    /// A stable lowercase label for diagnostics.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ScalarKind::Text => "text",
            ScalarKind::Number => "number",
            ScalarKind::Bool => "boolean",
            ScalarKind::Date => "date",
            ScalarKind::Datetime => "datetime",
        }
    }
}

/// One scalar frontmatter value.
#[derive(Debug, Clone, PartialEq)]
pub enum Scalar {
    /// A text value.
    Text(String),
    /// A finite binary64 number. Negative zero is normalized to zero at parse
    /// time and integers outside the exactly representable binary64 range are
    /// rejected rather than rounded.
    Number(f64),
    /// A boolean.
    Bool(bool),
    /// A calendar date.
    Date(Date),
    /// An explicit-offset datetime.
    Datetime(Datetime),
}

impl Scalar {
    /// The kind of this scalar.
    #[must_use]
    pub fn kind(&self) -> ScalarKind {
        match self {
            Scalar::Text(_) => ScalarKind::Text,
            Scalar::Number(_) => ScalarKind::Number,
            Scalar::Bool(_) => ScalarKind::Bool,
            Scalar::Date(_) => ScalarKind::Date,
            Scalar::Datetime(_) => ScalarKind::Datetime,
        }
    }
}

/// One frontmatter value: a scalar or a homogeneous list of scalars.
///
/// A list property remains a list even with one member; empty lists are
/// omitted rather than serialized.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A single scalar.
    Scalar(Scalar),
    /// A homogeneous, one-dimensional list of scalars of one kind.
    List(Vec<Scalar>),
}

/// The largest integer magnitude exactly representable in binary64 (2^53).
pub const MAX_EXACT_INTEGER: i128 = 9_007_199_254_740_992;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_kinds_are_reported() {
        assert_eq!(Scalar::Text(String::new()).kind(), ScalarKind::Text);
        assert_eq!(Scalar::Number(1.5).kind(), ScalarKind::Number);
        assert_eq!(Scalar::Bool(true).kind(), ScalarKind::Bool);
    }
}
