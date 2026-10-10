use kanna_detect::{store, Agent, Screen, Source, Status};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    rows: Vec<String>,
    #[serde(default)]
    title: String,
    #[serde(default)]
    progress: String,
    #[serde(default)]
    cli_version: Option<String>,
    expected: String,
    source: String,
}

fn bundled(name: &str) -> Agent {
    let (_, text) = store::bundled().find(|(n, _)| *n == name).unwrap_or_else(|| panic!("no bundled rules for {name}"));
    Agent::parse(text, Source::Bundled).unwrap_or_else(|e| panic!("{name}.json: {e}"))
}

#[test]
fn every_bundled_rule_file_loads() {
    for (name, text) in store::bundled() {
        let agent = Agent::parse(text, Source::Bundled).unwrap_or_else(|e| panic!("{name}.json: {e}"));
        assert_eq!(agent.name(), name);
    }
}

#[test]
fn corpus() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../agent-rules/corpus");
    let mut agents: Vec<PathBuf> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    agents.sort();
    let mut failures = vec![];
    let mut report = String::new();
    let mut total = 0;
    for dir in agents {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let agent = bundled(&name);
        let mut matrix: BTreeMap<(Status, Status), usize> = BTreeMap::new();
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
        files.sort();
        for file in files {
            let case: Case = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
            assert!(!case.source.trim().is_empty(), "{} needs a source", file.display());
            let expected = Status::parse(&case.expected).unwrap_or_else(|| panic!("{}: unknown status {}", file.display(), case.expected));
            let mut lines = case.rows;
            while lines.last().is_some_and(|l| l.trim().is_empty()) {
                lines.pop();
            }
            let screen = Screen { lines, title: case.title, progress: case.progress };
            let verdict = agent.evaluate(&screen, case.cli_version.as_deref());
            *matrix.entry((expected, verdict.status)).or_default() += 1;
            total += 1;
            if verdict.status != expected {
                failures.push(format!("{}: expected {}, got {} by {:?}", file.strip_prefix(&root).unwrap().display(), expected.as_str(), verdict.status.as_str(), verdict.rule));
            }
        }
        report += &format!("\n{name} (expected ↓, judged →)\n{:<11}", "");
        for actual in Status::ALL {
            report += &format!("{:>11}", actual.as_str());
        }
        report += "\n";
        for expected in Status::ALL {
            report += &format!("{:<11}", expected.as_str());
            for actual in Status::ALL {
                report += &format!("{:>11}", matrix.get(&(expected, actual)).copied().unwrap_or(0));
            }
            report += "\n";
        }
    }
    eprintln!("corpus: {total} screens{report}");
    assert!(failures.is_empty(), "{} of {total} screens judged wrong:\n{}", failures.len(), failures.join("\n"));
}
