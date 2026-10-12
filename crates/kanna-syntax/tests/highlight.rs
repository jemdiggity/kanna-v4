use kanna_syntax::{highlight, language_for, Scope, CAPTURES};

fn captured(language: &str, text: &str) -> Vec<(String, &'static str)> {
    let found = highlight(language, text).expect("a known language");
    let units: Vec<u16> = text.encode_utf16().collect();
    found.spans.iter().map(|s| (String::from_utf16_lossy(&units[s.start as usize..s.end as usize]), CAPTURES[s.capture as usize])).collect()
}

fn has(found: &[(String, &str)], text: &str, capture: &str) -> bool {
    found.iter().any(|(t, c)| t == text && (*c == capture || c.starts_with(&format!("{capture}."))))
}

#[test]
fn rust_keywords_functions_types_strings_and_macros_are_captured() {
    let found = captured("rust", include_str!("fixtures/sample.rs"));
    for (text, capture) in [("fn", "keyword"), ("read", "function"), ("usize", "type"), ("\"keeper\"", "string"), ("println!", "function"), ("LIMIT", "constructor"), ("16", "constant"), ("Reader", "type")] {
        assert!(has(&found, text, capture), "{text} as {capture}: {found:?}");
    }
}

#[test]
fn rust_scopes_are_functions_impls_loops_and_ifs() {
    let found = highlight("rust", include_str!("fixtures/sample.rs")).unwrap();
    assert_eq!(found.scopes, vec![Scope { first: 5, last: 13 }, Scope { first: 7, last: 12 }, Scope { first: 8, last: 10 }, Scope { first: 15, last: 19 }, Scope { first: 16, last: 18 }]);
}

#[test]
fn spans_count_utf16_units() {
    let text = "let crab = \"🦀\"; let n = 1;";
    let found = captured("rust", text);
    assert!(has(&found, "\"🦀\"", "string"), "{found:?}");
    assert!(has(&found, "1", "constant"), "{found:?}");
}

#[test]
fn languages_come_from_the_file_name() {
    assert_eq!(language_for("src/main.rs", ""), Some("rust"));
    assert_eq!(language_for("README", ""), None);
    assert_eq!(highlight("cobol", "x"), None);
}
