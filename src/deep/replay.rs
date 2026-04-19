//! Rust-side replay harness: read a `.dclog`, call the ported function on
//! each entry, compare outputs, report structured diffs on mismatch.

use std::fmt::Write as _;
use std::path::Path;

use super::reader::{LogEntry, LogReader};
use crate::spec::{Error as SpecError, Outcome, Result as SpecResult, Value};

pub struct EntryView<'e> {
    pub entry: &'e LogEntry,
}

impl<'e> EntryView<'e> {
    pub fn new(entry: &'e LogEntry) -> Self {
        Self { entry }
    }

    pub fn seq(&self) -> u32 {
        self.entry.seq
    }

    pub fn content_hash(&self) -> u64 {
        self.entry.content_hash
    }

    pub fn input(&self, name: &str) -> SpecResult<&'e Value> {
        self.entry
            .inputs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
            .ok_or_else(|| SpecError::MissingField(format!("input '{}'", name)))
    }

    pub fn output(&self, name: &str) -> SpecResult<&'e Value> {
        match &self.entry.outcome {
            Outcome::Return(outputs) => outputs
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v)
                .ok_or_else(|| SpecError::MissingField(format!("output '{}'", name))),
            Outcome::Exception { type_name, .. } => Err(SpecError::Other(format!(
                "expected Return outcome but entry raised {}",
                type_name
            ))),
        }
    }

    pub fn is_exception(&self) -> bool {
        matches!(self.entry.outcome, Outcome::Exception { .. })
    }

    pub fn outcome(&self) -> &Outcome {
        &self.entry.outcome
    }

    pub fn receiver_in(&self) -> Option<&'e Value> {
        self.entry.receiver_in.as_ref()
    }

    pub fn receiver_out(&self) -> Option<&'e Value> {
        self.entry.receiver_out.as_ref()
    }

    pub fn exception_type(&self) -> Option<&'e str> {
        match &self.entry.outcome {
            Outcome::Exception { type_name, .. } => Some(type_name),
            Outcome::Return(_) => None,
        }
    }

    pub fn exception_what(&self) -> Option<&'e str> {
        match &self.entry.outcome {
            Outcome::Exception { what, .. } => Some(what),
            Outcome::Return(_) => None,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Diff {
    pub mismatches: Vec<Mismatch>,
}

#[derive(Debug, Clone)]
pub struct Mismatch {
    pub path: String,
    pub message: String,
}

impl Diff {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.mismatches.push(Mismatch {
            path: path.into(),
            message: message.into(),
        });
    }

    pub fn is_empty(&self) -> bool {
        self.mismatches.is_empty()
    }

    pub fn extend(&mut self, other: Diff) {
        self.mismatches.extend(other.mismatches);
    }
}

#[derive(Debug)]
pub struct ReplayOutcome {
    pub seq: u32,
    pub content_hash: u64,
    pub diff: Option<Diff>,
    pub error: Option<SpecError>,
}

impl ReplayOutcome {
    pub fn is_ok(&self) -> bool {
        self.error.is_none() && self.diff.as_ref().map_or(true, |d| d.is_empty())
    }
}

#[derive(Debug, Default)]
pub struct ReplayReport {
    pub entries: Vec<ReplayOutcome>,
}

impl ReplayReport {
    pub fn total(&self) -> usize {
        self.entries.len()
    }
    pub fn passed(&self) -> usize {
        self.entries.iter().filter(|o| o.is_ok()).count()
    }
    pub fn failed(&self) -> usize {
        self.total() - self.passed()
    }

    pub fn render_failures(&self) -> String {
        let mut out = String::new();
        for o in &self.entries {
            if o.is_ok() {
                continue;
            }
            writeln!(
                &mut out,
                "=== entry seq={} hash=0x{:016x} ===",
                o.seq, o.content_hash
            )
            .unwrap();
            if let Some(e) = &o.error {
                writeln!(&mut out, "  replay error: {}", e).unwrap();
            }
            if let Some(d) = &o.diff {
                for m in &d.mismatches {
                    writeln!(&mut out, "  {}: {}", m.path, m.message).unwrap();
                }
            }
        }
        out
    }
}

pub fn replay<P, F>(path: P, mut f: F) -> SpecResult<ReplayReport>
where
    P: AsRef<Path>,
    F: FnMut(&EntryView<'_>) -> SpecResult<Diff>,
{
    let mut reader = LogReader::open(path)?;
    let mut report = ReplayReport::default();

    while let Some(entry) = reader.next_entry()? {
        let view = EntryView::new(&entry);
        let seq = view.seq();
        let hash = view.content_hash();
        let (diff, err) = match f(&view) {
            Ok(d) => (Some(d), None),
            Err(e) => (None, Some(e)),
        };
        report.entries.push(ReplayOutcome {
            seq,
            content_hash: hash,
            diff,
            error: err,
        });
    }
    Ok(report)
}

pub fn replay_dir<P, Filter, F>(
    dir: P,
    mut filter: Filter,
    mut body: F,
) -> SpecResult<Vec<(String, ReplayReport)>>
where
    P: AsRef<Path>,
    Filter: FnMut(&str) -> bool,
    F: FnMut(&str, &EntryView<'_>) -> SpecResult<Diff>,
{
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir.as_ref())? {
        let entry = entry?;
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("dclog") {
            continue;
        }
        let stem = p
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("<unknown>")
            .to_string();
        if !filter(&stem) {
            continue;
        }
        let report = replay(&p, |view| body(&stem, view))?;
        out.push((stem, report));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

pub fn replay_assert<P, F>(path: P, f: F)
where
    P: AsRef<Path>,
    F: FnMut(&EntryView<'_>) -> SpecResult<Diff>,
{
    let report = replay(path, f).expect("replay I/O error");
    if report.failed() == 0 {
        eprintln!("tracehash: all {} entries matched", report.passed());
        return;
    }
    let summary = report.render_failures();
    panic!(
        "tracehash: {}/{} entries mismatched\n{}",
        report.failed(),
        report.total(),
        summary
    );
}
