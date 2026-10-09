#![allow(dead_code)]

use kanna_mux::json::{encode, num, Value};
use kanna_mux::DEBUG_COMMANDS;
use std::path::PathBuf;

pub type Send = Box<dyn FnMut(Value) -> Value>;

pub fn cases_directory() -> PathBuf {
    let kmux = std::env::var_os("KMUX_DIR").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../kmux"));
    kmux.join("tests/kmux-protocol/cases")
}

pub fn mismatch(expected: &Value, actual: Option<&Value>, path: &str) -> Option<String> {
    let actual = actual.unwrap_or(&Value::Null);
    match expected {
        Value::Array(items) => {
            let Value::Array(got) = actual else { return Some(format!("{path}: expected an array, got {}", encode(actual))) };
            if got.len() != items.len() {
                return Some(format!("{path}: expected {} items, got {}", items.len(), got.len()));
            }
            items.iter().zip(got).enumerate().find_map(|(i, (item, got))| mismatch(item, Some(got), &format!("{path}[{i}]")))
        }
        Value::Object(fields) => {
            let Value::Object(got) = actual else { return Some(format!("{path}: expected an object, got {}", encode(actual))) };
            fields.iter().find_map(|(key, value)| mismatch(value, got.get(key), &if path.is_empty() { key.clone() } else { format!("{path}.{key}") }))
        }
        _ => (!kanna_mux::json::same(expected, actual)).then(|| format!("{path}: expected {}, got {}", encode(expected), encode(actual))),
    }
}

fn listed(reply: &Value, key: &str) -> Vec<String> {
    reply[key].as_array().map(|t| t.iter().filter_map(|t| t.as_str().map(String::from)).collect()).unwrap_or_default()
}

pub fn run_cases(label: &str, make: &dyn Fn() -> Send) -> usize {
    let mut files: Vec<PathBuf> = std::fs::read_dir(cases_directory())
        .expect("no cases directory")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no cases in {}", cases_directory().display());
    let capabilities = make()(serde_json::json!({ "id": 0, "cmd": "capabilities" }));
    let types = listed(&capabilities, "paneTypes");
    let commands = listed(&capabilities, "commands");

    let (mut passed, mut failed, mut skipped) = (0, vec![], vec![]);
    for file in &files {
        let name = file.file_name().unwrap().to_string_lossy();
        let cases: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        let Value::Array(cases) = cases else { panic!("{name} is not an array of cases") };
        for case in &cases {
            let title = format!("{name}: {}", case["name"].as_str().unwrap_or("?"));
            let Some(steps) = case["steps"].as_array() else { continue };
            let mut missing: Vec<String> = steps
                .iter()
                .filter_map(|step| {
                    let send = &step["send"];
                    let cmd = send["cmd"].as_str()?;
                    if !commands.iter().any(|c| c == cmd) && !DEBUG_COMMANDS.contains(&cmd) && cmd != "nope" {
                        return Some(cmd.to_string());
                    }
                    let kind = send["args"]["type"].as_str()?;
                    (cmd == "open" && ["term", "web", "ios", "md"].contains(&kind) && !types.iter().any(|t| t == kind)).then(|| format!("{kind} panes"))
                })
                .collect();
            if !missing.is_empty() {
                missing.sort();
                missing.dedup();
                skipped.push(format!("{title} (needs {})", missing.join(", ")));
                continue;
            }
            let mut send = make();
            let mut problem = None;
            for (index, step) in steps.iter().enumerate() {
                let Value::Object(mut request) = step["send"].clone() else { continue };
                request.insert("id".into(), num((index + 1) as f64));
                let reply = send(Value::Object(request));
                let expected = step.get("expect").cloned().unwrap_or(serde_json::json!({ "ok": true }));
                if let Some(why) = mismatch(&expected, Some(&reply), "") {
                    problem = Some(format!("{title}\n  step {}: {why}\n  reply: {}", index + 1, encode(&reply)));
                    break;
                }
            }
            match problem {
                Some(problem) => failed.push(problem),
                None => passed += 1,
            }
        }
    }
    println!("{label}: {passed} cases passed, {} failed, {} skipped", failed.len(), skipped.len());
    for skip in &skipped {
        println!("  skipped {skip}");
    }
    assert!(failed.is_empty(), "{} cases failed:\n{}", failed.len(), failed.join("\n"));
    passed
}
