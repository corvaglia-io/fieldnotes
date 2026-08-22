//! A minimal JSON writer for the stable machine-readable output.
//!
//! The CLI emits a small, fixed set of documented shapes, so it builds them
//! from this value tree instead of taking a serialization framework
//! dependency. String escaping reuses the format crate's RFC 8785 string
//! serializer, which is already the notebook contract's own rule.

use fieldnotes_format::jcs;

/// A JSON value.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    /// A string.
    Str(String),
    /// A non-negative integer.
    Int(u64),
    /// A boolean.
    Bool(bool),
    /// `null`.
    Null,
    /// An array.
    Arr(Vec<Json>),
    /// An object, emitted in insertion order so the shape is stable.
    Obj(Vec<(&'static str, Json)>),
}

impl Json {
    /// A string value.
    pub fn text(value: impl Into<String>) -> Json {
        Json::Str(value.into())
    }

    /// A string value, or `null` when absent.
    pub fn maybe_text(value: Option<impl Into<String>>) -> Json {
        value.map_or(Json::Null, |value| Json::Str(value.into()))
    }

    /// A count.
    pub fn count(value: usize) -> Json {
        Json::Int(u64::try_from(value).unwrap_or(u64::MAX))
    }

    /// Renders compact JSON.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Json::Str(value) => out.push_str(&jcs::serialize_string(value)),
            Json::Int(value) => out.push_str(&value.to_string()),
            Json::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Json::Null => out.push_str("null"),
            Json::Arr(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Json::Obj(entries) => {
                out.push('{');
                for (index, (key, value)) in entries.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    out.push_str(&jcs::serialize_string(key));
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Json;

    #[test]
    fn renders_stable_compact_json() {
        let value = Json::Obj(vec![
            ("schema", Json::text("fieldnotes.test.v1")),
            ("ok", Json::Bool(true)),
            ("count", Json::count(2)),
            ("name", Json::maybe_text(None::<String>)),
            ("items", Json::Arr(vec![Json::text("a\"b"), Json::Int(1)])),
        ]);
        assert_eq!(
            value.render(),
            r#"{"schema":"fieldnotes.test.v1","ok":true,"count":2,"name":null,"items":["a\"b",1]}"#
        );
    }
}
