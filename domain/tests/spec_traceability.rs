//! Traceability and governance gate for declarative behavior specifications (ADR-0019).
//!
//! Scans `backend/specs/domain/` and `backend/specs/api/` and enforces 4 automated checks:
//! 1. Metadata & Uniqueness: valid registered IDs (`VO-01`), statuses (`Draft`/`Approved`/`Deprecated`), and origins.
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

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum SpecSection {
    #[default]
    Other,
    Rules,
    OpenQuestions,
}

struct PendingHeading {
    id: String,
    status: Option<String>,
    origin: Option<String>,
}

#[derive(Default)]
struct SpecParseState {
    pending: Option<PendingHeading>,
    section: SpecSection,
    code_fence: Option<char>,
    rules: Vec<(String, RuleRecord)>,
    open_question_ids: BTreeSet<String>,
}

struct ParsedSpecs {
    registered_prefixes: BTreeSet<String>,
    rules: BTreeMap<String, RuleRecord>,
    open_question_ids: BTreeSet<String>,
}

#[test]
fn specs_and_tests_are_consistent() {
    let backend_root =
        backend_workspace_root().expect("domain crate must have a backend parent directory");
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

    let referenced = collect_test_refs(&backend_root, &parsed.registered_prefixes, &mut errors);
    verify_traceability(&parsed, &referenced, &mut errors);

    assert!(
        errors.is_empty(),
        "spec/test traceability violations:\n  - {}",
        errors.join("\n  - ")
    );
}

fn backend_workspace_root() -> Result<PathBuf, String> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "domain crate must have a backend parent directory".to_owned())
}

fn collect_spec_rules(specs_root: &Path, errors: &mut Vec<String>) -> ParsedSpecs {
    let registered_prefixes = parse_registered_prefixes(&specs_root.join("README.md"), errors);
    let mut spec_files = Vec::new();
    for layer in ["domain", "api"] {
        collect_files_with_ext(&specs_root.join(layer), "md", &mut spec_files);
    }

    let mut rules = BTreeMap::new();
    let mut open_question_ids = BTreeSet::new();
    for file in &spec_files {
        let (file_rules, file_open_qs) = parse_spec_file(file, errors);
        register_parsed_rules(file_rules, &registered_prefixes, &mut rules, errors);
        open_question_ids.extend(file_open_qs);
    }
    ParsedSpecs {
        registered_prefixes,
        rules,
        open_question_ids,
    }
}

fn parse_registered_prefixes(readme_path: &Path, errors: &mut Vec<String>) -> BTreeSet<String> {
    let Ok(text) = fs::read_to_string(readme_path) else {
        errors.push(format!(
            "cannot read prefix registry at {}",
            readme_path.display()
        ));
        return BTreeSet::new();
    };
    let prefixes = extract_registry_prefixes(&text);
    if prefixes.is_empty() {
        errors.push(format!(
            "no registered prefixes found in {}",
            readme_path.display()
        ));
    }
    prefixes
}

fn extract_registry_prefixes(text: &str) -> BTreeSet<String> {
    let mut prefixes = BTreeSet::new();
    let mut in_registry = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            in_registry = trimmed.eq_ignore_ascii_case("## Prefix Registry");
            continue;
        }
        if !in_registry || !trimmed.starts_with('|') {
            continue;
        }
        if let Some(cell) = trimmed.split('|').nth(1) {
            let prefix = cell.trim().trim_matches('`');
            if is_valid_prefix(prefix) {
                prefixes.insert(prefix.to_owned());
            }
        }
    }
    prefixes
}

fn register_parsed_rules(
    parsed: Vec<(String, RuleRecord)>,
    registered_prefixes: &BTreeSet<String>,
    rules: &mut BTreeMap<String, RuleRecord>,
    errors: &mut Vec<String>,
) {
    for (id, record) in parsed {
        validate_rule_metadata(&id, &record, registered_prefixes, errors);
        insert_unique_rule(id, record, rules, errors);
    }
}

fn validate_rule_metadata(
    id: &str,
    record: &RuleRecord,
    registered_prefixes: &BTreeSet<String>,
    errors: &mut Vec<String>,
) {
    if let Some((prefix, _)) = id.split_once('-')
        && !registered_prefixes.is_empty()
        && !registered_prefixes.contains(prefix)
    {
        errors.push(format!(
            "{}: rule {id} uses unregistered prefix `{prefix}` (register it in specs/README.md)",
            record.file.display(),
        ));
    }
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
    let Ok(text) = fs::read_to_string(path) else {
        errors.push(format!("cannot read spec file {}", path.display()));
        return (Vec::new(), BTreeSet::new());
    };
    parse_spec_text(&text, path, errors)
}

fn parse_spec_text(
    text: &str,
    path: &Path,
    errors: &mut Vec<String>,
) -> (Vec<(String, RuleRecord)>, BTreeSet<String>) {
    let mut state = SpecParseState::default();
    for line in text.lines() {
        parse_spec_line(line, path, &mut state, errors);
    }
    flush_pending_heading(path, &mut state, errors);
    (state.rules, state.open_question_ids)
}

fn parse_spec_line(line: &str, path: &Path, state: &mut SpecParseState, errors: &mut Vec<String>) {
    if toggle_code_fence(line, &mut state.code_fence) || state.code_fence.is_some() {
        return;
    }
    let trimmed = line.trim();
    if trimmed.starts_with("## ") {
        flush_pending_heading(path, state, errors);
        state.section = classify_section(trimmed);
        return;
    }
    if trimmed == "---" {
        flush_pending_heading(path, state, errors);
        return;
    }
    match state.section {
        SpecSection::Rules => parse_rule_line(line, path, state, errors),
        SpecSection::OpenQuestions => {
            scan_open_question_header(trimmed, &mut state.open_question_ids)
        }
        SpecSection::Other => {}
    }
}

fn toggle_code_fence(line: &str, fence: &mut Option<char>) -> bool {
    let trimmed = line.trim_start();
    let marker = if trimmed.starts_with("```") {
        Some('`')
    } else if trimmed.starts_with("~~~") {
        Some('~')
    } else {
        None
    };
    let Some(ch) = marker else {
        return false;
    };
    *fence = match *fence {
        Some(active) if active == ch => None,
        Some(active) => Some(active),
        None => Some(ch),
    };
    true
}

fn classify_section(heading: &str) -> SpecSection {
    if heading.eq_ignore_ascii_case("## Rules") {
        SpecSection::Rules
    } else if heading.eq_ignore_ascii_case("## Open Questions") {
        SpecSection::OpenQuestions
    } else {
        SpecSection::Other
    }
}

fn parse_rule_line(line: &str, path: &Path, state: &mut SpecParseState, errors: &mut Vec<String>) {
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

fn scan_open_question_header(trimmed: &str, open_question_ids: &mut BTreeSet<String>) {
    if !is_open_question_item(trimmed) {
        return;
    }
    let header = trimmed.split_once(':').map_or(trimmed, |(h, _)| h);
    for token in header.split(|c: char| !c.is_ascii_alphanumeric() && c != '-') {
        if is_valid_rule_id(token) {
            open_question_ids.insert(token.to_owned());
        }
    }
}

fn is_open_question_item(trimmed: &str) -> bool {
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with('#') {
        return true;
    }
    let digits_end = trimmed.bytes().take_while(|b| b.is_ascii_digit()).count();
    digits_end > 0 && trimmed.as_bytes().get(digits_end) == Some(&b'.')
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

fn collect_test_refs(
    backend_root: &Path,
    registered_prefixes: &BTreeSet<String>,
    errors: &mut Vec<String>,
) -> BTreeSet<String> {
    let mut test_files = Vec::new();
    for dir in collect_test_dirs(backend_root) {
        collect_files_with_ext(&dir, "rs", &mut test_files);
    }
    test_files.retain(|p| p.file_name().is_some_and(|n| n != "spec_traceability.rs"));

    let mut referenced = BTreeSet::new();
    for file in &test_files {
        let Ok(text) = fs::read_to_string(file) else {
            errors.push(format!("cannot read test file {}", file.display()));
            continue;
        };
        scan_rust_test_text(&text, registered_prefixes, &mut referenced);
    }
    referenced
}

fn scan_rust_test_text(
    text: &str,
    registered_prefixes: &BTreeSet<String>,
    referenced: &mut BTreeSet<String>,
) {
    scan_rust_string_literals(text, registered_prefixes, referenced);
    scan_fn_rule_ids(text, registered_prefixes, referenced);
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

fn scan_rust_string_literals(
    text: &str,
    registered_prefixes: &BTreeSet<String>,
    referenced: &mut BTreeSet<String>,
) {
    let bytes = text.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx..].starts_with(b"//") {
            idx = skip_line_comment(bytes, idx);
        } else if bytes[idx..].starts_with(b"/*") {
            idx = skip_block_comment(bytes, idx);
        } else if bytes[idx] == b'\'' {
            idx = skip_char_literal_or_lifetime(text, idx);
        } else if let Some(next_idx) = scan_raw_string(text, idx, registered_prefixes, referenced) {
            idx = next_idx;
        } else if bytes[idx] == b'"' {
            idx = scan_normal_string(text, idx, registered_prefixes, referenced);
        } else {
            idx += 1;
        }
    }
}

fn skip_line_comment(bytes: &[u8], start: usize) -> usize {
    let mut idx = start + 2;
    while idx < bytes.len() && bytes[idx] != b'\n' {
        idx += 1;
    }
    idx
}

fn skip_block_comment(bytes: &[u8], start: usize) -> usize {
    let mut idx = start + 2;
    let mut depth = 1usize;
    while idx < bytes.len() && depth > 0 {
        if bytes[idx..].starts_with(b"/*") {
            depth += 1;
            idx += 2;
        } else if bytes[idx..].starts_with(b"*/") {
            depth -= 1;
            idx += 2;
        } else {
            idx += 1;
        }
    }
    idx
}

fn skip_char_literal_or_lifetime(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    if bytes.get(start + 1) == Some(&b'\\') {
        let mut idx = start + 2;
        if idx < bytes.len() {
            idx += 1;
        }
        while idx < bytes.len() && bytes[idx] != b'\'' && bytes[idx] != b'\n' {
            idx += 1;
        }
        return if bytes.get(idx) == Some(&b'\'') {
            idx + 1
        } else {
            start + 1
        };
    }
    let Some(ch) = text[start + 1..].chars().next() else {
        return start + 1;
    };
    let close_idx = start + 1 + ch.len_utf8();
    if bytes.get(close_idx) == Some(&b'\'') {
        close_idx + 1
    } else {
        start + 1
    }
}

fn scan_raw_string(
    text: &str,
    start: usize,
    registered_prefixes: &BTreeSet<String>,
    referenced: &mut BTreeSet<String>,
) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes[start] != b'r' {
        return None;
    }
    let mut p = start + 1;
    while bytes.get(p) == Some(&b'#') {
        p += 1;
    }
    if bytes.get(p) != Some(&b'"') {
        return None;
    }
    let hashes = p - (start + 1);
    let content_start = p + 1;
    let end_marker = format!("\"{}", "#".repeat(hashes));
    let rel_end = text[content_start..].find(&end_marker)?;
    let content = &text[content_start..content_start + rel_end];
    record_if_registered_id(content, registered_prefixes, referenced);
    Some(content_start + rel_end + end_marker.len())
}

fn scan_normal_string(
    text: &str,
    start: usize,
    registered_prefixes: &BTreeSet<String>,
    referenced: &mut BTreeSet<String>,
) -> usize {
    let bytes = text.as_bytes();
    let content_start = start + 1;
    let mut idx = content_start;
    while idx < bytes.len() {
        if bytes[idx] == b'\\' {
            idx += 2;
            continue;
        }
        if bytes[idx] == b'"' {
            let content = &text[content_start..idx];
            record_if_registered_id(content, registered_prefixes, referenced);
            return idx + 1;
        }
        idx += 1;
    }
    bytes.len()
}

fn record_if_registered_id(
    candidate: &str,
    registered_prefixes: &BTreeSet<String>,
    referenced: &mut BTreeSet<String>,
) {
    if is_registered_rule_id(candidate, registered_prefixes) {
        referenced.insert(candidate.to_owned());
    }
}

fn scan_fn_rule_ids(
    text: &str,
    registered_prefixes: &BTreeSet<String>,
    referenced: &mut BTreeSet<String>,
) {
    let mut has_test_attr = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        if trimmed.starts_with("#[") {
            has_test_attr |= is_test_attribute(trimmed);
            continue;
        }
        if let Some(after_fn) = strip_fn_prefix(trimmed) {
            if has_test_attr {
                record_fn_name_id(after_fn, registered_prefixes, referenced);
            }
            has_test_attr = false;
            continue;
        }
        has_test_attr = false;
    }
}

fn is_test_attribute(trimmed: &str) -> bool {
    trimmed == "#[test]"
        || trimmed.starts_with("#[tokio::test")
        || trimmed.starts_with("#[rstest")
        || trimmed.starts_with("#[async_std::test")
}

fn strip_fn_prefix(trimmed: &str) -> Option<&str> {
    trimmed
        .strip_prefix("fn ")
        .or_else(|| trimmed.strip_prefix("async fn "))
        .or_else(|| trimmed.strip_prefix("pub fn "))
        .or_else(|| trimmed.strip_prefix("pub async fn "))
}

fn record_fn_name_id(
    after_fn: &str,
    registered_prefixes: &BTreeSet<String>,
    referenced: &mut BTreeSet<String>,
) {
    let name_end = after_fn
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .unwrap_or(after_fn.len());
    if let Some(id) = parse_fn_prefix_id(&after_fn[..name_end])
        && is_registered_rule_id(&id, registered_prefixes)
    {
        referenced.insert(id);
    }
}

fn parse_fn_prefix_id(fn_name: &str) -> Option<String> {
    let mut parts = fn_name.split('_');
    let prefix = parts.next()?;
    let num = parts.next()?;
    let valid_prefix =
        (2..=4).contains(&prefix.len()) && prefix.bytes().all(|b| b.is_ascii_lowercase());
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

fn is_valid_prefix(token: &str) -> bool {
    (2..=4).contains(&token.len()) && token.bytes().all(|b| b.is_ascii_uppercase())
}

fn is_valid_rule_id(token: &str) -> bool {
    let Some((prefix, num)) = token.split_once('-') else {
        return false;
    };
    let valid_num = (2..=3).contains(&num.len()) && num.bytes().all(|b| b.is_ascii_digit());
    is_valid_prefix(prefix) && valid_num
}

fn is_registered_rule_id(token: &str, registered_prefixes: &BTreeSet<String>) -> bool {
    let Some((prefix, _)) = token.split_once('-') else {
        return false;
    };
    is_valid_rule_id(token) && registered_prefixes.contains(prefix)
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

#[cfg(test)]
mod guardrail_tests {
    use super::*;

    fn rule(status: &str, origin: &str) -> RuleRecord {
        RuleRecord {
            status: status.to_owned(),
            origin: origin.to_owned(),
            file: PathBuf::from("specs/domain/sample.md"),
        }
    }

    fn prefixes() -> BTreeSet<String> {
        BTreeSet::from(["VO".to_owned(), "FM".to_owned()])
    }

    #[test]
    fn blocks_test_referencing_draft_rule() {
        let mut rules = BTreeMap::new();
        rules.insert("VO-01".to_owned(), rule("Draft", "human"));
        let parsed = ParsedSpecs {
            registered_prefixes: prefixes(),
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
            registered_prefixes: prefixes(),
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
            registered_prefixes: prefixes(),
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
            registered_prefixes: prefixes(),
            rules,
            open_question_ids: BTreeSet::new(),
        };
        let referenced = BTreeSet::from(["VO-01".to_owned()]);
        let mut errors = Vec::new();
        verify_traceability(&parsed, &referenced, &mut errors);
        assert!(errors.is_empty());
    }

    #[test]
    fn ignores_unregistered_prefixes_and_non_test_helpers() {
        let code = r#"
            fn http_404_helper() { let _ = "UTF-16"; let _ = "API-404"; }
            fn vo_01_not_a_test() {}
            #[test]
            fn http_404_returns_not_found() {}
            #[test]
            #[ignore]
            fn vo_02_actual_test() {}
            #[tokio::test]
            async fn fm_01_async_test() {}
        "#;
        let mut referenced = BTreeSet::new();
        scan_rust_test_text(code, &prefixes(), &mut referenced);
        assert_eq!(
            referenced,
            BTreeSet::from(["VO-02".to_owned(), "FM-01".to_owned()])
        );
    }

    #[test]
    fn handles_char_literals_escapes_comments_and_raw_strings() {
        let code = r#####"
            // "VO-01" in line comment must be ignored
            /* "VO-02" in /* nested */ block comment must be ignored */
            const QUOTE_CHAR: char = '"';
            const ESC_CHAR: char = '\'';
            const AFTER_QUOTE: &str = "escaped \"quote\" before";
            const REAL_REF: &str = "VO-03";
            const RAW_REF: &str = r#"FM-01"#;
        "#####;
        let mut referenced = BTreeSet::new();
        scan_rust_test_text(code, &prefixes(), &mut referenced);
        assert_eq!(
            referenced,
            BTreeSet::from(["VO-03".to_owned(), "FM-01".to_owned()])
        );
    }

    #[test]
    fn ignores_code_fences_findings_tables_and_open_question_prose_refs() {
        let md = r#"
# Title

## Vocabulary
| Term | Symbol | Value |
| :--- | :--- | :--- |
| `VO-99` | `Fake` | Ignore outside Rules |

## Rules
```markdown
| `VO-98` | `Approved` | `human` |
```

| ID | Status | Origin | Given | When | Then |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `FM-01` | `Draft` | `human` | a | b | c |

## Open Questions
1. **Q1 (`FM-07`)**: Unlike `FM-01`, should this be allowed?
   Also check `VO-01` in continuation prose.

---

## Findings
| # | Location | Code | Doc | Impact |
| :--- | :--- | :--- | :--- | :--- |
| `VO-97` | `Draft` | `human` | a | b |
"#;
        let mut errors = Vec::new();
        let (rules, open_qs) = parse_spec_text(md, Path::new("sample.md"), &mut errors);
        assert!(errors.is_empty());
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].0, "FM-01");
        assert_eq!(open_qs, BTreeSet::from(["FM-07".to_owned()]));
    }

    #[test]
    fn rejects_unregistered_prefix_in_spec() {
        let mut errors = Vec::new();
        register_parsed_rules(
            vec![("ZZ-01".to_owned(), rule("Draft", "human"))],
            &prefixes(),
            &mut BTreeMap::new(),
            &mut errors,
        );
        assert!(errors.iter().any(|e| e.contains("unregistered prefix")));
    }
}
