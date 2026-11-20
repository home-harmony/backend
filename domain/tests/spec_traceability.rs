//! Traceability and governance gate for declarative behavior specifications (ADR-0019).
//!
//! Scans `backend/specs/domain/` and `backend/specs/api/` and enforces 4 automated checks:
//! 1. Metadata & Uniqueness: valid IDs (`VO-01`), statuses (`Draft`/`Approved`/`Deprecated`), and origins.
//! 2. Block Premature Tests: tests may ONLY reference `Approved` rules (never `Draft` or `Deprecated`).
//! 3. Block Unconfirmed Approvals: `Approved` rules cannot have `Origin: observed` or remain in `## Open Questions`.
//! 4. 1:1 Coverage: every `Approved` rule has a test in `backend/*/tests/` and every test citation exists in specs.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const VALID_STATUSES: [&str; 3] = ["Draft", "Approved", "Deprecated"];

#[derive(Clone)]
struct RuleRecord {
    status: String,
    origin: String,
    file: PathBuf,
}

struct PendingHeading {
    id: String,
    status: Option<String>,
    origin: Option<String>,
}

#[derive(Default)]
struct SpecParseState {
    pending: Option<PendingHeading>,
    in_open_questions: bool,
    rules: Vec<(String, RuleRecord)>,
    open_question_ids: BTreeSet<String>,
}

struct ParsedSpecs {
    rules: BTreeMap<String, RuleRecord>,
    open_question_ids: BTreeSet<String>,
}

#[test]
fn specs_and_tests_are_consistent() {
    let backend_root = backend_workspace_root();
    let specs_root = backend_root.join("specs");
    assert!(
        specs_root.is_dir(),
        "expected specs directory at {}",
        specs_root.display()
    );

    let mut errors = Vec::new();
    let parsed = collect_spec_rules(&specs_root, &mut errors);
    assert!(
        !parsed.rules.is_empty(),
        "expected at least one behavior spec rule under {}",
        specs_root.display()
    );

    let referenced = collect_test_refs(&backend_root);
    verify_traceability(&parsed, &referenced, &mut errors);

    assert!(
        errors.is_empty(),
        "spec/test traceability violations:\n  - {}",
        errors.join("\n  - ")
    );
}

fn backend_workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("domain crate must have a backend parent directory")
        .to_path_buf()
}

fn collect_spec_rules(specs_root: &Path, errors: &mut Vec<String>) -> ParsedSpecs {
    let mut spec_files = Vec::new();
    for layer in ["domain", "api"] {
        collect_files_with_ext(&specs_root.join(layer), "md", &mut spec_files);
    }

    let mut rules = BTreeMap::new();
    let mut open_question_ids = BTreeSet::new();
    for file in &spec_files {
        let (file_rules, file_open_qs) = parse_spec_file(file, errors);
        register_parsed_rules(file_rules, &mut rules, errors);
        open_question_ids.extend(file_open_qs);
    }
    ParsedSpecs {
        rules,
        open_question_ids,
    }
}

fn register_parsed_rules(
    parsed: Vec<(String, RuleRecord)>,
    rules: &mut BTreeMap<String, RuleRecord>,
    errors: &mut Vec<String>,
) {
    for (id, record) in parsed {
        validate_rule_metadata(&id, &record, errors);
        insert_unique_rule(id, record, rules, errors);
    }
}

fn validate_rule_metadata(id: &str, record: &RuleRecord, errors: &mut Vec<String>) {
    if !VALID_STATUSES.contains(&record.status.as_str()) {
        errors.push(format!(
            "{}: rule {id} has unknown status `{}` (expected {VALID_STATUSES:?})",
            record.file.display(),
            record.status
        ));
    }
    if !is_valid_origin(&record.origin) {
        errors.push(format!(
            "{}: rule {id} has invalid origin `{}` (expected human, observed, adr:..., or doc:...)",
            record.file.display(),
            record.origin
        ));
    }
}

fn insert_unique_rule(
    id: String,
    record: RuleRecord,
    rules: &mut BTreeMap<String, RuleRecord>,
    errors: &mut Vec<String>,
) {
    if let Some(prev) = rules.get(&id) {
        errors.push(format!(
            "duplicate rule ID {id}: {} and {}",
            prev.file.display(),
            record.file.display()
        ));
        return;
    }
    rules.insert(id, record);
}

fn parse_spec_file(
    path: &Path,
    errors: &mut Vec<String>,
) -> (Vec<(String, RuleRecord)>, BTreeSet<String>) {
    let text = fs::read_to_string(path).expect("spec file must be readable");
    let mut state = SpecParseState::default();

    for line in text.lines() {
        parse_spec_line(line, path, &mut state, errors);
    }
    flush_pending_heading(path, &mut state, errors);
    (state.rules, state.open_question_ids)
}

fn parse_spec_line(
    line: &str,
    path: &Path,
    state: &mut SpecParseState,
    errors: &mut Vec<String>,
) {
    if is_section_heading(line) {
        flush_pending_heading(path, state, errors);
        state.in_open_questions = line.trim().eq_ignore_ascii_case("## Open Questions");
        return;
    }
    if state.in_open_questions {
        scan_open_question_ids(line, &mut state.open_question_ids);
        return;
    }
    parse_rule_line(line, path, state, errors);
}

fn parse_rule_line(
    line: &str,
    path: &Path,
    state: &mut SpecParseState,
    errors: &mut Vec<String>,
) {
    if let Some(token) = extract_heading_token(line) {
        handle_heading_token(token, path, state, errors);
        return;
    }
    if let Some(status) = extract_field_value(line, "- **Status**:") {
        set_pending_status(status, state);
        return;
    }
    if let Some(origin) = extract_field_value(line, "- **Origin**:") {
        set_pending_origin(origin, state);
        return;
    }
    if let Some((id, status, origin)) = extract_table_rule(line) {
        flush_pending_heading(path, state, errors);
        record_table_rule(id, status, origin, path, state, errors);
    }
}

fn handle_heading_token(
    token: &str,
    path: &Path,
    state: &mut SpecParseState,
    errors: &mut Vec<String>,
) {
    if is_valid_rule_id(token) {
        flush_pending_heading(path, state, errors);
        state.pending = Some(PendingHeading {
            id: token.to_owned(),
            status: None,
            origin: None,
        });
        return;
    }
    if is_loose_rule_id(token) {
        errors.push(format!(
            "{}: malformed rule ID `{token}` (expected format e.g. `VO-01`)",
            path.display()
        ));
    }
}

fn set_pending_status(status: String, state: &mut SpecParseState) {
    if let Some(pending) = state.pending.as_mut() {
        pending.status = Some(status);
    }
}

fn set_pending_origin(origin: String, state: &mut SpecParseState) {
    if let Some(pending) = state.pending.as_mut() {
        pending.origin = Some(origin);
    }
}

fn flush_pending_heading(path: &Path, state: &mut SpecParseState, errors: &mut Vec<String>) {
    let Some(pending) = state.pending.take() else {
        return;
    };
    let Some(status) = pending.status else {
        errors.push(format!(
            "{}: rule {} is missing a `- **Status**:` line",
            path.display(),
            pending.id
        ));
        return;
    };
    let Some(origin) = pending.origin else {
        errors.push(format!(
            "{}: rule {} is missing a `- **Origin**:` line",
            path.display(),
            pending.id
        ));
        return;
    };
    state.rules.push((
        pending.id,
        RuleRecord {
            status,
            origin,
            file: path.to_path_buf(),
        },
    ));
}

fn record_table_rule(
    id: String,
    status: String,
    origin: String,
    path: &Path,
    state: &mut SpecParseState,
    errors: &mut Vec<String>,
) {
    if !is_valid_rule_id(&id) {
        errors.push(format!(
            "{}: malformed table rule ID `{id}` (expected e.g. `FM-01`)",
            path.display()
        ));
        return;
    }
    state.rules.push((
        id,
        RuleRecord {
            status,
            origin,
            file: path.to_path_buf(),
        },
    ));
}

fn is_section_heading(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("## ") || trimmed == "---"
}

fn scan_open_question_ids(line: &str, open_question_ids: &mut BTreeSet<String>) {
    for token in line.split(|c: char| !c.is_ascii_alphanumeric() && c != '-') {
        if is_valid_rule_id(token) {
            open_question_ids.insert(token.to_owned());
        }
    }
}

fn extract_heading_token(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let rest = trimmed
        .strip_prefix("#### ")
        .or_else(|| trimmed.strip_prefix("### "))?;
    let first = rest.split_whitespace().next()?;
    Some(first.trim_matches('`'))
}

fn extract_field_value(line: &str, prefix: &str) -> Option<String> {
    let rest = line.trim_start().strip_prefix(prefix)?;
    let token = rest.split_whitespace().next()?;
    Some(token.trim_matches('`').to_owned())
}

fn extract_table_rule(line: &str) -> Option<(String, String, String)> {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') {
        return None;
    }
    let mut cells = trimmed.split('|').skip(1);
    let id = cells.next()?.trim().trim_matches('`');
    if !(is_valid_rule_id(id) || is_loose_rule_id(id)) {
        return None;
    }
    let status = cells.next()?.trim().trim_matches('`');
    let origin = cells.next().map_or("", |c| c.trim().trim_matches('`'));
    Some((id.to_owned(), status.to_owned(), origin.to_owned()))
}

fn collect_test_refs(backend_root: &Path) -> BTreeSet<String> {
    let mut test_files = Vec::new();
    for dir in collect_test_dirs(backend_root) {
        collect_files_with_ext(&dir, "rs", &mut test_files);
    }
    test_files.retain(|p| p.file_name().is_some_and(|n| n != "spec_traceability.rs"));

    let mut referenced = BTreeSet::new();
    for file in &test_files {
        let text = fs::read_to_string(file).expect("test file must be readable");
        scan_quoted_rule_ids(&text, &mut referenced);
        scan_fn_rule_ids(&text, &mut referenced);
    }
    referenced
}

fn collect_test_dirs(backend_root: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![
        backend_root.join("domain").join("tests"),
        backend_root.join("infrastructure").join("tests"),
        backend_root.join("api").join("tests"),
    ];
    let Ok(lambdas) = fs::read_dir(backend_root.join("lambdas")) else {
        return dirs;
    };
    for entry in lambdas.flatten() {
        let path = entry.path();
        if path.is_dir() {
            dirs.push(path.join("tests"));
        }
    }
    dirs
}

fn scan_quoted_rule_ids(text: &str, referenced: &mut BTreeSet<String>) {
    let bytes = text.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] != b'"' {
            idx += 1;
            continue;
        }
        let start = idx + 1;
        let Some(rel_end) = text[start..].find('"') else {
            break;
        };
        let candidate = &text[start..start + rel_end];
        if is_valid_rule_id(candidate) {
            referenced.insert(candidate.to_owned());
        }
        idx = start + rel_end + 1;
    }
}

fn scan_fn_rule_ids(text: &str, referenced: &mut BTreeSet<String>) {
    for line in text.lines() {
        let trimmed = line.trim_start();
        let after_fn = trimmed
            .strip_prefix("fn ")
            .or_else(|| trimmed.strip_prefix("async fn "))
            .or_else(|| trimmed.strip_prefix("pub fn "))
            .or_else(|| trimmed.strip_prefix("pub async fn "));
        let Some(after_fn) = after_fn else {
            continue;
        };
        let name_end = after_fn
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(after_fn.len());
        if let Some(id) = parse_fn_prefix_id(&after_fn[..name_end]) {
            referenced.insert(id);
        }
    }
}

fn parse_fn_prefix_id(fn_name: &str) -> Option<String> {
    let mut parts = fn_name.split('_');
    let prefix = parts.next()?;
    let num = parts.next()?;
    let valid_prefix = (2..=4).contains(&prefix.len())
        && prefix.bytes().all(|b| b.is_ascii_lowercase());
    let valid_num = (2..=3).contains(&num.len()) && num.bytes().all(|b| b.is_ascii_digit());
    if valid_prefix && valid_num {
        return Some(format!("{}-{num}", prefix.to_ascii_uppercase()));
    }
    None
}

fn verify_traceability(
    parsed: &ParsedSpecs,
    referenced: &BTreeSet<String>,
    errors: &mut Vec<String>,
) {
    check_approved_rules(parsed, referenced, errors);
    check_referenced_rules(parsed, referenced, errors);
}

fn check_approved_rules(
    parsed: &ParsedSpecs,
    referenced: &BTreeSet<String>,
    errors: &mut Vec<String>,
) {
    for (id, record) in &parsed.rules {
        if record.status != "Approved" {
            continue;
        }
        if record.origin == "observed" {
            errors.push(format!(
                "Approved rule {id} ({}) still has 'Origin: observed' — human must confirm intent by setting 'Origin: human', 'adr:NNNN', or 'doc:<path>'",
                record.file.display()
            ));
        }
        if parsed.open_question_ids.contains(id) {
            errors.push(format!(
                "Approved rule {id} ({}) is still listed in Open Questions",
                record.file.display()
            ));
        }
        if !referenced.contains(id) {
            errors.push(format!(
                "Approved rule {id} ({}) has no test: cite \"{id}\" or name test `{}_...`",
                record.file.display(),
                id.to_ascii_lowercase().replace('-', "_")
            ));
        }
    }
}

fn check_referenced_rules(
    parsed: &ParsedSpecs,
    referenced: &BTreeSet<String>,
    errors: &mut Vec<String>,
) {
    for id in referenced {
        let Some(record) = parsed.rules.get(id) else {
            errors.push(format!(
                "tests reference rule {id}, which does not exist in any spec"
            ));
            continue;
        };
        if record.status != "Approved" {
            errors.push(format!(
                "test references rule {id} with status '{}' — only 'Approved' rules may have spec tests",
                record.status
            ));
        }
    }
}

fn is_valid_origin(origin: &str) -> bool {
    origin == "human"
        || origin == "observed"
        || origin.strip_prefix("adr:").is_some_and(|s| !s.is_empty())
        || origin.strip_prefix("doc:").is_some_and(|s| !s.is_empty())
}

fn is_valid_rule_id(token: &str) -> bool {
    let Some((prefix, num)) = token.split_once('-') else {
        return false;
    };
    let valid_prefix =
        (2..=4).contains(&prefix.len()) && prefix.bytes().all(|b| b.is_ascii_uppercase());
    let valid_num = (2..=3).contains(&num.len()) && num.bytes().all(|b| b.is_ascii_digit());
    valid_prefix && valid_num
}

fn is_loose_rule_id(token: &str) -> bool {
    let Some((prefix, num)) = token.split_once('-') else {
        return false;
    };
    let alpha_prefix =
        (1..=6).contains(&prefix.len()) && prefix.bytes().all(|b| b.is_ascii_alphabetic());
    let digit_num = (1..=6).contains(&num.len()) && num.bytes().all(|b| b.is_ascii_digit());
    alpha_prefix && digit_num
}

fn collect_files_with_ext(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files_with_ext(&path, ext, out);
        } else if path.extension().is_some_and(|e| e == ext) {
            out.push(path);
        }
    }
}

mod guardrail_tests {
    use super::*;

    fn rule(status: &str, origin: &str) -> RuleRecord {
        RuleRecord {
            status: status.to_owned(),
            origin: origin.to_owned(),
            file: PathBuf::from("specs/domain/sample.md"),
        }
    }

    #[test]
    fn blocks_test_referencing_draft_rule() {
        let mut rules = BTreeMap::new();
        rules.insert("VO-01".to_owned(), rule("Draft", "human"));
        let parsed = ParsedSpecs {
            rules,
            open_question_ids: BTreeSet::new(),
        };
        let referenced = BTreeSet::from(["VO-01".to_owned()]);
        let mut errors = Vec::new();
        verify_traceability(&parsed, &referenced, &mut errors);
        assert!(errors.iter().any(|e| e.contains("only 'Approved' rules")));
    }

    #[test]
    fn blocks_approved_rule_with_observed_origin() {
        let mut rules = BTreeMap::new();
        rules.insert("VO-01".to_owned(), rule("Approved", "observed"));
        let parsed = ParsedSpecs {
            rules,
            open_question_ids: BTreeSet::new(),
        };
        let referenced = BTreeSet::from(["VO-01".to_owned()]);
        let mut errors = Vec::new();
        verify_traceability(&parsed, &referenced, &mut errors);
        assert!(errors.iter().any(|e| e.contains("Origin: observed")));
    }

    #[test]
    fn blocks_approved_rule_in_open_questions() {
        let mut rules = BTreeMap::new();
        rules.insert("FM-07".to_owned(), rule("Approved", "human"));
        let parsed = ParsedSpecs {
            rules,
            open_question_ids: BTreeSet::from(["FM-07".to_owned()]),
        };
        let referenced = BTreeSet::from(["FM-07".to_owned()]);
        let mut errors = Vec::new();
        verify_traceability(&parsed, &referenced, &mut errors);
        assert!(errors.iter().any(|e| e.contains("Open Questions")));
    }

    #[test]
    fn accepts_approved_confirmed_rule_with_test() {
        let mut rules = BTreeMap::new();
        rules.insert("VO-01".to_owned(), rule("Approved", "human"));
        let parsed = ParsedSpecs {
            rules,
            open_question_ids: BTreeSet::new(),
        };
        let referenced = BTreeSet::from(["VO-01".to_owned()]);
        let mut errors = Vec::new();
        verify_traceability(&parsed, &referenced, &mut errors);
        assert!(errors.is_empty());
    }
}
