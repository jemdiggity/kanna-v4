use kanna_syntax::{highlight, language_for, Scope, CAPTURES};

fn captured(language: &str, text: &str) -> Vec<(String, &'static str)> {
    let found = highlight(language, text).expect("a known language");
    let units: Vec<u16> = text.encode_utf16().collect();
    found.spans.iter().map(|s| (String::from_utf16_lossy(&units[s.start as usize..s.end as usize]), CAPTURES[s.capture as usize])).collect()
}

fn has(found: &[(String, &str)], text: &str, capture: &str) -> bool {
    found.iter().any(|(t, c)| t.trim() == text && (*c == capture || c.starts_with(&format!("{capture}."))))
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

#[test]
fn every_language_compiles_its_queries() {
    assert_eq!(kanna_syntax::problems(), Vec::<String>::new());
}

struct Sample {
    language: &'static str,
    text: &'static str,
    captures: &'static [(&'static str, &'static str)],
    first_scope: u32,
}

const SAMPLES: &[Sample] = &[
    Sample { language: "swift", text: "import Foundation\n\nfunc greet(name: String) -> String {\n    if name.isEmpty {\n        return \"nobody\"\n    }\n    return name\n}\n", captures: &[("func", "keyword"), ("\"nobody\"", "string")], first_scope: 3 },
    Sample { language: "typescript", text: "interface Pane {\n  id: string;\n}\n\nfunction open(pane: Pane): number {\n  return 42;\n}\n", captures: &[("interface", "keyword"), ("function", "keyword"), ("42", "number")], first_scope: 1 },
    Sample { language: "tsx", text: "export function View(props: { name: string }) {\n  return (\n    <div className=\"x\">\n      {props.name}\n    </div>\n  );\n}\n", captures: &[("function", "keyword"), ("div", "tag"), ("\"x\"", "string")], first_scope: 1 },
    Sample { language: "javascript", text: "const add = (a, b) => {\n  return a + b;\n};\n\nclass Box {\n  open() {\n    return 'open';\n  }\n}\n", captures: &[("const", "keyword"), ("'open'", "string"), ("class", "keyword")], first_scope: 1 },
    Sample { language: "python", text: "import os\n\nclass Reader:\n    def read(self, path):\n        if path:\n            return open(path)\n        return None\n", captures: &[("def", "keyword"), ("class", "keyword"), ("None", "constant")], first_scope: 3 },
    Sample { language: "go", text: "package main\n\nfunc main() {\n\tfor i := 0; i < 3; i++ {\n\t\tprintln(\"go\")\n\t}\n}\n", captures: &[("func", "keyword"), ("\"go\"", "string"), ("package", "keyword")], first_scope: 3 },
    Sample { language: "c", text: "#include <stdio.h>\n\nint main(void) {\n    if (1) {\n        printf(\"c\\n\");\n    }\n    return 0;\n}\n", captures: &[("return", "keyword"), ("int", "type"), ("#include", "keyword")], first_scope: 3 },
    Sample { language: "cpp", text: "namespace kanna {\nclass Pane {\npublic:\n    int id() const {\n        return 1;\n    }\n};\n}\n", captures: &[("class", "keyword"), ("namespace", "keyword"), ("return", "keyword")], first_scope: 1 },
    Sample { language: "bash", text: "#!/bin/bash\nbuild() {\n  if [ -n \"$1\" ]; then\n    echo \"$1\"\n  fi\n}\n", captures: &[("if", "keyword"), ("echo", "function")], first_scope: 2 },
    Sample { language: "json", text: "{\n  \"name\": \"kanna\",\n  \"deps\": {\n    \"x\": 1\n  }\n}\n", captures: &[("\"kanna\"", "string"), ("1", "number")], first_scope: 3 },
    Sample { language: "toml", text: "[package]\nname = \"kanna\"\nversion = \"0.1.0\"\n\n[dependencies]\nx = 1\n", captures: &[("\"kanna\"", "string"), ("name", "property")], first_scope: 1 },
    Sample { language: "yaml", text: "server:\n  port: 8080\n  name: kanna\n", captures: &[("server", "property"), ("8080", "number")], first_scope: 1 },
    Sample { language: "html", text: "<html>\n  <body>\n    <script>\n      const x = 1;\n    </script>\n  </body>\n</html>\n", captures: &[("html", "tag"), ("const", "keyword")], first_scope: 1 },
    Sample { language: "css", text: ".pane {\n  color: #fff;\n  margin: 4px;\n}\n", captures: &[("color", "property")], first_scope: 1 },
    Sample { language: "svelte", text: "<script>\n  let count = 0;\n</script>\n\n<button on:click={() => count++}>\n  {count}\n</button>\n", captures: &[("let", "keyword"), ("button", "tag")], first_scope: 1 },
    Sample { language: "vue", text: "<template>\n  <div class=\"pane\">{{ title }}</div>\n</template>\n\n<script lang=\"ts\">\nconst title: string = 'kanna';\n</script>\n\n<style>\n.pane { color: red; }\n</style>\n", captures: &[("div", "tag"), ("const", "keyword"), ("'kanna'", "string"), ("color", "property")], first_scope: 1 },
];

#[test]
fn every_language_highlights_its_sample_and_finds_its_scopes() {
    for sample in SAMPLES {
        let found = captured(sample.language, sample.text);
        for (text, capture) in sample.captures {
            assert!(has(&found, text, capture), "{}: {text} as {capture}: {found:?}", sample.language);
        }
        let scopes = highlight(sample.language, sample.text).unwrap().scopes;
        assert_eq!(scopes.first().map(|s| s.first), Some(sample.first_scope), "{}: {scopes:?}", sample.language);
    }
}

#[test]
fn file_names_and_shebangs_pick_the_language() {
    for (name, first, language) in [
        ("a.swift", "", "swift"), ("a.ts", "", "typescript"), ("App.tsx", "", "tsx"), ("a.jsx", "", "javascript"), ("a.py", "", "python"),
        ("main.go", "", "go"), ("a.h", "", "c"), ("a.hpp", "", "cpp"), ("build.sh", "", "bash"), (".zshrc", "", "bash"), ("package.json", "", "json"),
        ("Cargo.toml", "", "toml"), ("ci.yml", "", "yaml"), ("index.html", "", "html"), ("a.css", "", "css"), ("App.svelte", "", "svelte"), ("App.vue", "", "vue"),
        ("build", "#!/usr/bin/env bash", "bash"), ("tool", "#!/usr/bin/python3", "python"), ("serve", "#!/usr/bin/env -S node --flag", "javascript"),
    ] {
        assert_eq!(language_for(name, first), Some(language), "{name}");
    }
}

#[test]
fn multiline_declarations_and_literals_are_scopes() {
    let text = "pub const LANGUAGES: &[Language] = &[\n    Language {\n        name: \"rust\",\n    },\n];\nconst ONE: u32 = 1;\n";
    let found = highlight("rust", text).unwrap();
    assert_eq!(found.scopes, vec![Scope { first: 1, last: 5 }, Scope { first: 2, last: 4 }]);
    let text = "const table = {\n  rows: [\n    1,\n  ],\n};\n";
    assert_eq!(highlight("typescript", text).unwrap().scopes, vec![Scope { first: 1, last: 5 }, Scope { first: 2, last: 4 }]);
}
