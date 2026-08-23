//! Shared loading and construction helpers for the graph test suites.
//!
//! Each integration-test binary compiles this module separately, so a helper one
//! suite does not use would otherwise be reported as dead code.
#![allow(dead_code)]

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use fieldnotes_domain::{
    Datetime, FieldId, FieldStemRegistry, NoteType, RecordId, RecordIdGenerator,
};
use fieldnotes_format::{ParsedRecord, RecordBuilder, parse_record, validate_record};
use fieldnotes_graph::{DerivedGraph, GraphConfig, derive_graph};
use fieldnotes_test_support::{CountingRandom, FixedClock};

/// A boxed-error test result.
pub type TestResult = Result<(), Box<dyn Error>>;

/// The instant every test's injected clock reports: `2026-08-22T12:10:00+02:00`,
/// the same generation instant the frozen entity fixtures carry.
pub const GENERATED_MILLIS: u64 = 1_787_393_400_000;

/// The instance ID the frozen corpus uses.
pub const INSTANCE_ID: &str = "fn_01a02837-2de0-7a2b-8c41-f2481851192a";

/// The frozen approved notebook corpus.
pub fn corpus_root() -> PathBuf {
    fieldnotes_test_support::fixtures_root()
        .join("notebooks")
        .join("proposed-v1")
}

/// Parses and validates every `.md` file directly inside `directory`, in
/// ascending filename order.
pub fn load_records(directory: &Path) -> Result<Vec<ParsedRecord>, Box<dyn Error>> {
    let mut paths: Vec<PathBuf> = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .collect();
    paths.sort();
    let mut records = Vec::new();
    for path in paths {
        let bytes = fs::read(&path)?;
        let record = parse_record(&bytes)?;
        validate_record(&record)?;
        records.push(record);
    }
    Ok(records)
}

/// The frozen corpus Notes.
pub fn corpus_notes() -> Result<Vec<ParsedRecord>, Box<dyn Error>> {
    load_records(&corpus_root().join("notes"))
}

/// The corpus Notes whose IDs end with one of `suffixes`, in that order.
pub fn corpus_notes_with_suffixes(suffixes: &[&str]) -> Result<Vec<ParsedRecord>, Box<dyn Error>> {
    let all = corpus_notes()?;
    let mut selected = Vec::new();
    for suffix in suffixes {
        let found = all
            .iter()
            .find(|record| record.id().to_string().ends_with(suffix))
            .ok_or_else(|| format!("no corpus Note ends with {suffix}"))?;
        selected.push(found.clone());
    }
    Ok(selected)
}

/// The standard test configuration: the v0.1 namespaces, a `+02:00` generation
/// offset, and no evidence bound.
pub fn config() -> GraphConfig {
    GraphConfig {
        generated_at_offset_minutes: 120,
        ..GraphConfig::default()
    }
}

/// Derives a graph with a fixed clock and a deterministic ID generator, so the
/// projection is byte-reproducible.
pub fn derive(
    records: &[ParsedRecord],
    config: &GraphConfig,
) -> Result<DerivedGraph, Box<dyn Error>> {
    let clock = FixedClock(GENERATED_MILLIS);
    let mut ids = RecordIdGenerator::new(FixedClock(GENERATED_MILLIS), CountingRandom::new(1));
    Ok(derive_graph(records, config, &clock, &mut ids)?)
}

/// The complete projected byte stream, as `path` plus record bytes, for
/// byte-identity comparisons.
pub fn projected_bytes(graph: &DerivedGraph) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    for projected in graph.projected_records()? {
        out.push_str(&projected.relative_path);
        out.push('\n');
        out.push_str(projected.record.text());
        out.push('\n');
    }
    Ok(out)
}

/// Builds a synthetic Note through the canonical builder, so no test ever
/// hand-formats notebook bytes.
///
/// `lists` values are `|`-separated list members; `scalars` values are single
/// text scalars.
pub fn note(
    note_id: &str,
    field_id: &str,
    note_type: NoteType,
    occurred_at: &str,
    body: &str,
    lists: &[(&str, &str)],
    scalars: &[(&str, &str)],
) -> Result<ParsedRecord, Box<dyn Error>> {
    let id = RecordId::parse(note_id)?;
    let instance = RecordId::parse(INSTANCE_ID)?;
    let field = FieldId::parse(field_id, FieldStemRegistry::v1())?;
    let mut builder = RecordBuilder::note(
        &id,
        &instance,
        &field,
        note_type,
        Datetime::parse(occurred_at)?,
    );
    builder.set_body(body);
    for (key, value) in lists {
        builder.set_text_list(key, value.split('|').collect::<Vec<&str>>());
    }
    for (key, value) in scalars {
        builder.set_text(key, *value);
    }
    Ok(builder.build()?.record().clone())
}
