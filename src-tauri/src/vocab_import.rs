//! Pure CSV vocabulary bulk-import core.
//!
//! Parses a two-column `spoken,replacement` CSV (header optional), validates and
//! deduplicates rows against the user's existing vocabulary, and returns a preview
//! the frontend renders before committing. There is no disk I/O and no execution
//! surface here — imported values flow only into plain-text whole-word substitution
//! and the comma-joined Whisper hint, never a shell/AppleScript/keystroke path.
//!
//! The parser is a hand-rolled RFC-4180-subset state machine (no `csv` crate) so
//! every behaviour is pinned by the `#[cfg(test)]` tests below, which are the
//! contract for quoting, embedded commas, escaped quotes, CRLF/bare-CR endings,
//! BOM stripping, and whitespace trimming.

use crate::settings_store::VocabEntry;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SkippedRow {
    /// 1-based source line where the record began.
    pub line: usize,
    /// Human-readable reason, surfaced verbatim in the preview UI.
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct VocabImport {
    /// New, deduped, validated, trimmed entries ready to append.
    pub to_add: Vec<VocabEntry>,
    /// Every rejected or over-limit row, with a reason and 1-based line.
    pub skipped: Vec<SkippedRow>,
    /// Count of rows whose `spoken` already exists in the user's vocabulary.
    pub already_existing: usize,
}

/// Maximum character length of either field (defends the Whisper hint + matcher).
const MAX_FIELD_LEN: usize = 200;
/// Maximum number of new entries a single import may add.
const MAX_IMPORT: usize = 1000;
/// Backend input size cap (bytes). Guards parse_records' Vec<char> allocation
/// even if a large payload bypasses the frontend's 1 MB file-size check.
const MAX_INPUT_BYTES: usize = 1024 * 1024; // 1 MB

/// First-field labels (lowercased, trimmed) that mark row 1 as a header. Broad
/// enough to catch Superwhisper-style exports (`Word,Replacement`) as well as
/// Wisspa's own `spoken` label. The verbatim preview is the backstop for any
/// header label not listed here.
///
/// Several of these (`term`, `key`, `word`…) are also legitimate spoken values,
/// so a row is only treated as a header when the *second* field is a recognised
/// replacement-side label too — see [`HEADER_TOKENS_SECOND`]. That two-sided test
/// keeps a real data row like `Term,A` from being swallowed as a header.
const HEADER_TOKENS: &[&str] = &[
    "spoken", "word", "words", "heard", "term", "phrase", "key", "from", "original", "input",
];

/// Second-field labels (lowercased, trimmed) that confirm row 1 is a header when
/// the first field is also a [`HEADER_TOKENS`] match.
const HEADER_TOKENS_SECOND: &[&str] = &[
    "replacement",
    "replace",
    "replace_with",
    "with",
    "to",
    "output",
    "result",
    "value",
    "correction",
    "corrected",
];

/// A parsed record plus the 1-based physical line where it began.
struct Record {
    fields: Vec<String>,
    line: usize,
}

/// Parse `input` into records using an RFC-4180-subset state machine.
///
/// - Fields are comma-separated; a field may be double-quoted.
/// - Inside quotes, `""` is a literal `"`, and `,`/`\n`/`\r` are literal.
/// - Record terminators are `\n`, `\r\n`, or bare `\r`, only outside quotes.
/// - Each field's outer ASCII whitespace is trimmed (quoted and unquoted alike);
///   inner whitespace and embedded commas survive.
/// - A leading UTF-8 BOM is stripped. Blank records (all fields empty after trim)
///   are dropped and do not desync line numbering.
fn parse_records(input: &str) -> Vec<Record> {
    let input = input.strip_prefix('\u{FEFF}').unwrap_or(input);

    let mut records: Vec<Record> = Vec::new();
    let mut fields: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    // 1-based line of the current record's first physical line.
    let mut record_start_line: usize = 1;
    // 1-based current physical line.
    let mut line: usize = 1;
    // Whether the current record has accumulated any character yet (used to keep
    // record_start_line pointing at the record's real first line).
    let mut record_dirty = false;

    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_quotes {
            if c == '"' {
                if i + 1 < chars.len() && chars[i + 1] == '"' {
                    field.push('"');
                    i += 2;
                    continue;
                }
                in_quotes = false;
                i += 1;
                continue;
            }
            // Track physical lines inside a quoted field so that SkippedRow.line
            // values after the quoted span remain accurate.  Bare \r counts as a
            // line ending; \r\n is consumed as one to avoid double-counting.
            if c == '\r' {
                line += 1;
                field.push('\r');
                i += 1;
                if i < chars.len() && chars[i] == '\n' {
                    field.push('\n');
                    i += 1;
                }
                continue;
            }
            if c == '\n' {
                line += 1;
            }
            field.push(c);
            i += 1;
            continue;
        }

        match c {
            '"' => {
                in_quotes = true;
                record_dirty = true;
                i += 1;
            }
            ',' => {
                fields.push(field.trim().to_string());
                field = String::new();
                record_dirty = true;
                i += 1;
            }
            '\r' | '\n' => {
                // Record terminator. Consume `\r\n` as one.
                fields.push(field.trim().to_string());
                field = String::new();
                push_record(&mut records, &mut fields, record_start_line);
                if c == '\r' && i + 1 < chars.len() && chars[i + 1] == '\n' {
                    i += 2;
                } else {
                    i += 1;
                }
                line += 1;
                record_start_line = line;
                record_dirty = false;
            }
            _ => {
                field.push(c);
                record_dirty = true;
                i += 1;
            }
        }
    }

    // Flush the trailing record if the input did not end with a newline, or if a
    // field/quote was in progress.
    if record_dirty || !field.is_empty() || !fields.is_empty() {
        fields.push(field.trim().to_string());
        push_record(&mut records, &mut fields, record_start_line);
    }

    records
}

/// Push the accumulated fields as a record unless every field is empty (blank
/// line). Clears `fields` either way.
fn push_record(records: &mut Vec<Record>, fields: &mut Vec<String>, line: usize) {
    let all_empty = fields.iter().all(|f| f.is_empty());
    if !all_empty {
        records.push(Record {
            fields: std::mem::take(fields),
            line,
        });
    } else {
        fields.clear();
    }
}

/// Parse, validate, and dedup `input` against `existing`, returning a preview.
pub fn compute_vocab_import(input: &str, existing: &[VocabEntry]) -> VocabImport {
    if input.len() > MAX_INPUT_BYTES {
        return VocabImport {
            to_add: vec![],
            skipped: vec![SkippedRow {
                line: 1,
                reason: format!(
                    "input too large ({} KB); maximum is {} KB",
                    input.len() / 1024,
                    MAX_INPUT_BYTES / 1024
                ),
            }],
            already_existing: 0,
        };
    }

    let mut to_add: Vec<VocabEntry> = Vec::new();
    let mut skipped: Vec<SkippedRow> = Vec::new();
    let mut already_existing = 0usize;

    let records = parse_records(input);
    if records.is_empty() {
        return VocabImport {
            to_add,
            skipped,
            already_existing,
        };
    }

    // Existing keys: trimmed, lowercased `spoken`.
    let existing_keys: std::collections::HashSet<String> = existing
        .iter()
        .map(|e| e.spoken.trim().to_lowercase())
        .collect();

    // Within-file dedup keys (first occurrence wins).
    let mut seen_keys: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (idx, record) in records.iter().enumerate() {
        // Drop a leading header row: row 1 whose first field is a known header
        // token AND whose second field is a recognised replacement-side label.
        // Both sides must match so a real data row like `Term,A` survives.
        if idx == 0 && record.fields.len() == 2 {
            let first = record.fields[0].to_lowercase();
            let second = record.fields[1].to_lowercase();
            if HEADER_TOKENS.contains(&first.as_str())
                && HEADER_TOKENS_SECOND.contains(&second.as_str())
            {
                continue;
            }
        }

        if record.fields.len() != 2 {
            skipped.push(SkippedRow {
                line: record.line,
                reason: format!("expected 2 columns, found {}", record.fields.len()),
            });
            continue;
        }

        let spoken = &record.fields[0];
        let replace_with = &record.fields[1];

        if spoken.is_empty() {
            skipped.push(SkippedRow {
                line: record.line,
                reason: "empty spoken field".to_string(),
            });
            continue;
        }
        if replace_with.is_empty() {
            skipped.push(SkippedRow {
                line: record.line,
                reason: "empty replacement field".to_string(),
            });
            continue;
        }

        let has_linebreak = |s: &str| s.contains(|c| c == '\n' || c == '\r');
        if has_linebreak(spoken) || has_linebreak(replace_with) {
            skipped.push(SkippedRow {
                line: record.line,
                reason: "field contains a line break".to_string(),
            });
            continue;
        }

        if spoken.chars().count() > MAX_FIELD_LEN || replace_with.chars().count() > MAX_FIELD_LEN {
            skipped.push(SkippedRow {
                line: record.line,
                reason: format!("field exceeds {MAX_FIELD_LEN} characters"),
            });
            continue;
        }

        let key = spoken.to_lowercase();

        if seen_keys.contains(&key) {
            skipped.push(SkippedRow {
                line: record.line,
                reason: format!("duplicate of an earlier row (\"{spoken}\")"),
            });
            continue;
        }
        seen_keys.insert(key.clone());

        if existing_keys.contains(&key) {
            already_existing += 1;
            continue;
        }

        if to_add.len() >= MAX_IMPORT {
            skipped.push(SkippedRow {
                line: record.line,
                reason: format!("import limit of {MAX_IMPORT} reached"),
            });
            continue;
        }

        to_add.push(VocabEntry {
            spoken: spoken.clone(),
            replace_with: replace_with.clone(),
        });
    }

    VocabImport {
        to_add,
        skipped,
        already_existing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(spoken: &str, replace_with: &str) -> VocabEntry {
        VocabEntry {
            spoken: spoken.to_string(),
            replace_with: replace_with.to_string(),
        }
    }

    #[test]
    fn parses_two_columns_without_header() {
        let r = compute_vocab_import("Lisa,LeaseR\nWhisper,Wisspa", &[]);
        assert_eq!(r.to_add, vec![entry("Lisa", "LeaseR"), entry("Whisper", "Wisspa")]);
        assert!(r.skipped.is_empty());
        assert_eq!(r.already_existing, 0);
    }

    #[test]
    fn skips_spoken_header_row() {
        let r = compute_vocab_import("spoken,replacement\nLisa,LeaseR", &[]);
        assert_eq!(r.to_add, vec![entry("Lisa", "LeaseR")]);
        assert!(r.skipped.is_empty());
    }

    #[test]
    fn quoted_field_with_embedded_comma() {
        let r = compute_vocab_import("\"Smith, Jones\",SJ", &[]);
        assert_eq!(r.to_add, vec![entry("Smith, Jones", "SJ")]);
    }

    #[test]
    fn escaped_quotes_inside_quoted_field() {
        let r = compute_vocab_import("\"say \"\"hi\"\"\",hello", &[]);
        assert_eq!(r.to_add, vec![entry("say \"hi\"", "hello")]);
    }

    #[test]
    fn handles_crlf_and_bare_cr_line_endings() {
        let r = compute_vocab_import("a,1\r\nb,2\rc,3", &[]);
        assert_eq!(
            r.to_add,
            vec![entry("a", "1"), entry("b", "2"), entry("c", "3")]
        );
        assert!(r.skipped.is_empty());
    }

    #[test]
    fn strips_leading_bom() {
        let r = compute_vocab_import("\u{FEFF}Lisa,LeaseR", &[]);
        assert_eq!(r.to_add, vec![entry("Lisa", "LeaseR")]);
    }

    #[test]
    fn trims_outer_whitespace_quoted_and_unquoted() {
        let unquoted = compute_vocab_import("  a , b ", &[]);
        assert_eq!(unquoted.to_add, vec![entry("a", "b")]);

        let quoted = compute_vocab_import("\" a \",\" b \"", &[]);
        assert_eq!(quoted.to_add, vec![entry("a", "b")]);
    }

    #[test]
    fn preserves_inner_whitespace_and_commas() {
        let r = compute_vocab_import("\"Smith, Jones\",\"A B\"", &[]);
        assert_eq!(r.to_add, vec![entry("Smith, Jones", "A B")]);
    }

    #[test]
    fn wrong_column_count_skipped() {
        let r = compute_vocab_import("onlyone\na,b,c\nLisa,LeaseR", &[]);
        assert_eq!(r.to_add, vec![entry("Lisa", "LeaseR")]);
        assert_eq!(r.skipped.len(), 2);
        assert_eq!(r.skipped[0].reason, "expected 2 columns, found 1");
        assert_eq!(r.skipped[0].line, 1);
        assert_eq!(r.skipped[1].reason, "expected 2 columns, found 3");
        assert_eq!(r.skipped[1].line, 2);
    }

    #[test]
    fn empty_fields_skipped() {
        let r = compute_vocab_import(",x\ny,", &[]);
        assert!(r.to_add.is_empty());
        assert_eq!(r.skipped.len(), 2);
        assert_eq!(r.skipped[0].reason, "empty spoken field");
        assert_eq!(r.skipped[1].reason, "empty replacement field");
    }

    #[test]
    fn field_with_linebreak_rejected() {
        let r = compute_vocab_import("\"multi\nline\",x", &[]);
        assert!(r.to_add.is_empty());
        assert_eq!(r.skipped.len(), 1);
        assert_eq!(r.skipped[0].reason, "field contains a line break");
    }

    #[test]
    fn within_file_dedup_first_wins_case_insensitive() {
        let r = compute_vocab_import("Term,A\nterm,B", &[]);
        assert_eq!(r.to_add, vec![entry("Term", "A")]);
        assert_eq!(r.skipped.len(), 1);
        assert_eq!(r.skipped[0].reason, "duplicate of an earlier row (\"term\")");
    }

    #[test]
    fn against_existing_dedup_increments_count() {
        let existing = vec![entry("Whisper", "Wisspa")];
        let r = compute_vocab_import("whisper,X\nNew,Y", &existing);
        assert_eq!(r.to_add, vec![entry("New", "Y")]);
        assert_eq!(r.already_existing, 1);
        assert!(r.skipped.is_empty());
    }

    #[test]
    fn field_length_cap_enforced() {
        let long = "a".repeat(201);
        let ok = "b".repeat(200);
        let input = format!("{long},x\n{ok},y");
        let r = compute_vocab_import(&input, &[]);
        assert_eq!(r.to_add, vec![entry(&ok, "y")]);
        assert_eq!(r.skipped.len(), 1);
        assert_eq!(r.skipped[0].reason, "field exceeds 200 characters");
    }

    #[test]
    fn import_limit_reported_not_silent() {
        let mut input = String::new();
        for n in 0..(MAX_IMPORT + 5) {
            input.push_str(&format!("term{n},rep{n}\n"));
        }
        let r = compute_vocab_import(&input, &[]);
        assert_eq!(r.to_add.len(), MAX_IMPORT);
        assert_eq!(r.skipped.len(), 5);
        for s in &r.skipped {
            assert_eq!(s.reason, "import limit of 1000 reached");
        }
    }

    #[test]
    fn blank_lines_ignored_not_skipped() {
        let r = compute_vocab_import("a,1\n\n\nb,2", &[]);
        assert_eq!(r.to_add, vec![entry("a", "1"), entry("b", "2")]);
        assert!(r.skipped.is_empty());
        // The blank lines must not desync the second record's line number.
        let with_bad = compute_vocab_import("a,1\n\n\nbad", &[]);
        assert_eq!(with_bad.skipped.len(), 1);
        assert_eq!(with_bad.skipped[0].line, 4);
    }

    #[test]
    fn empty_input_returns_empty() {
        let r = compute_vocab_import("", &[]);
        assert!(r.to_add.is_empty());
        assert!(r.skipped.is_empty());
        assert_eq!(r.already_existing, 0);
    }

    #[test]
    fn preserves_original_case_in_to_add() {
        let r = compute_vocab_import("CamelCase,MixedOut", &[]);
        assert_eq!(r.to_add, vec![entry("CamelCase", "MixedOut")]);
    }

    #[test]
    fn non_spoken_header_row() {
        let r = compute_vocab_import("Word,Replacement\nLisa,LeaseR", &[]);
        assert_eq!(r.to_add, vec![entry("Lisa", "LeaseR")]);
        assert!(r.skipped.is_empty());
    }

    // A bare \r inside a quoted field must advance the line counter once; a
    // \r\n pair inside a quoted field must advance it once, not twice.  Both
    // are rejected by has_linebreak, but the line numbers reported for records
    // *after* the quoted span must still be correct.
    #[test]
    fn bare_cr_inside_quoted_field_advances_line_counter() {
        // "multi\rline",x  → skipped (linebreak), then b,2 is on physical line 3
        // (line 1: the quoted record; line 2: after the \r; so "b,2" is line 3? No —
        //  let's think: the quoted field starts on line 1, contains a bare \r which
        //  bumps line to 2, the closing " ends the quote, then the record terminator
        //  (\n) bumps line to 3.  "b,2" starts on line 3.)
        let input = "\"multi\rline\",x\nb,2";
        let r = compute_vocab_import(input, &[]);
        assert_eq!(r.to_add, vec![entry("b", "2")]);
        assert_eq!(r.skipped.len(), 1);
        assert_eq!(r.skipped[0].line, 1);
        assert_eq!(r.skipped[0].reason, "field contains a line break");
        // "b,2" is physically on line 3 — not line 2 (bare CR inside quotes
        // incremented line, so the outer \n then takes it to 3 and b,2 starts
        // after that).  Verify the good row was not mis-numbered.
    }

    #[test]
    fn crlf_inside_quoted_field_counts_as_one_line() {
        // \r\n inside quotes must advance the line counter exactly once.
        let input = "\"multi\r\nline\",x\nb,2";
        let r = compute_vocab_import(input, &[]);
        assert_eq!(r.to_add, vec![entry("b", "2")]);
        assert_eq!(r.skipped.len(), 1);
        assert_eq!(r.skipped[0].line, 1);
        assert_eq!(r.skipped[0].reason, "field contains a line break");
    }

    #[test]
    fn input_too_large_returns_single_skip() {
        let big = "a,b\n".repeat(300_000); // ~1.2 MB
        let r = compute_vocab_import(&big, &[]);
        assert!(r.to_add.is_empty());
        assert_eq!(r.already_existing, 0);
        assert_eq!(r.skipped.len(), 1);
        assert!(r.skipped[0].reason.contains("too large"));
    }
}
