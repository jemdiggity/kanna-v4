#![cfg(kmux_reference)]

extern crate kanna_client as kmux_client;

#[allow(dead_code)]
mod output {
    include!(concat!(env!("OUT_DIR"), "/kmux_output.rs"));
}

#[allow(dead_code)]
mod commands {
    include!(concat!(env!("OUT_DIR"), "/kmux_commands.rs"));
}

use kanna::args::Argv;
use serde_json::{json, Value};

const KANNA_ONLY: &[&str] = &["read", "status", "markup", "rename-pane", "task"];

const EXTENDED: &[(&str, &str)] = &[("open", "notes"), ("send", "notes"), ("list", "notes")];

fn from_kmux(text: &str) -> String {
    text.replace("kmux", "kanna")
}

fn from_kanna(text: &str) -> String {
    text.replace("Kanna.app", "kanna").replace("Kanna", "kanna")
}

fn words(line: &str) -> Vec<String> {
    let mut words = vec![];
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for c in line.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => word.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                started = true;
            }
            (None, ' ') => {
                if started || !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
                started = false;
            }
            (None, c) => word.push(c),
        }
    }
    if started || !word.is_empty() {
        words.push(word);
    }
    words
}

fn kmux_parse(line: &str) -> Result<Value, (u8, String)> {
    let words = words(line);
    let mut args = kmux_client::Args::new(words[1..].to_vec());
    args.flag("--json");
    let name = args.positional().unwrap();
    let command = commands::find(&name).ok_or((2, format!("unknown command {name}")))?;
    let request = (command.parse)(&mut args).map_err(|f| (f.code, f.message))?;
    if let Some(extra) = args.first() {
        let f = commands::usage(command.name, &format!("unexpected argument \"{extra}\""));
        return Err((f.code, f.message));
    }
    Ok(request)
}

fn kanna_parse(line: &str) -> Result<Value, (u8, String)> {
    let words = words(&from_kmux(line));
    let mut args = Argv::new(words[1..].to_vec());
    args.flag("--json");
    let name = args.positional().unwrap();
    let command = kanna::commands::find(&name).ok_or((2, format!("unknown command {name}")))?;
    let request = (command.parse)(&mut args).map_err(|f| (f.code, f.message))?;
    if let Some(extra) = args.first() {
        let f = kanna::commands::usage(command.name, &format!("unexpected argument \"{extra}\""));
        return Err((f.code, f.message));
    }
    Ok(request)
}

#[test]
fn kanna_has_every_kmux_command_in_kmux_order() {
    let kmux: Vec<(&str, &str)> = commands::COMMANDS.iter().map(|c| (c.name, c.group)).collect();
    let kanna: Vec<(&str, &str)> = kanna::commands::COMMANDS.iter().filter(|c| !KANNA_ONLY.contains(&c.name)).map(|c| (c.name, c.group)).collect();
    assert_eq!(kanna, kmux);
    assert_eq!(kanna::commands::GROUPS, commands::GROUPS);
}

#[test]
fn every_command_is_documented_as_kmux_documents_it() {
    for theirs in commands::COMMANDS {
        let ours = kanna::commands::find(theirs.name).unwrap();
        let fields = [
            ("summary", theirs.summary.to_string(), ours.summary.to_string()),
            ("usage", theirs.usage.to_string(), ours.usage.to_string()),
            ("notes", theirs.notes.to_string(), ours.notes.to_string()),
            ("options", format!("{:?}", theirs.options), format!("{:?}", ours.options)),
            ("examples", format!("{:?}", theirs.examples), format!("{:?}", ours.examples)),
        ];
        for (field, kmux, kanna) in fields {
            let (kmux, kanna) = (from_kmux(&kmux), from_kanna(&kanna));
            if EXTENDED.contains(&(theirs.name, field)) {
                assert!(kanna.starts_with(&kmux) && kanna.len() > kmux.len(), "{} {field}: kanna should add to kmux's\nkmux:  {kmux}\nkanna: {kanna}", theirs.name);
            } else {
                assert_eq!(kanna, kmux, "{} {field}", theirs.name);
            }
        }
    }
}

#[test]
fn every_kmux_example_and_request_parses_the_same() {
    let mut lines: Vec<String> = commands::COMMANDS.iter().flat_map(|c| c.examples.iter().map(|(e, _)| e.to_string())).collect();
    lines.extend(
        [
            "kmux open",
            "kmux open --name logs --split right --size 1/3 --cmd 'tail -f a.log'",
            "kmux open web localhost:3000 --split down",
            "kmux open --tab --no-wait",
            "kmux open --window w2 --name x",
            "kmux close logs",
            "kmux close --window w2",
            "kmux close --tab t3",
            "kmux focus --window w1",
            "kmux rename-tab t1 my servers",
            "kmux send server npm test",
            "kmux send server ls --color",
            "kmux move logs --to server --side bottom",
            "kmux move logs --tab new --window w2",
            "kmux move-tab t2 --index 0",
            "kmux move-tab t2 --window=new",
            "kmux open web a.test --history",
            "kmux navigate site --back",
            "kmux navigate site --forward",
            "kmux navigate site https://x.dev/a.md",
            "kmux open md /tmp/a.md",
            "kmux navigate spec /tmp/b.md",
            "kmux open ios --app com.apple.Preferences --device 'iPhone 16'",
            "kmux resize logs 25%",
            "kmux arrange '{\"split\":\"column\",\"children\":[{\"pane\":\"a\"}]}' --window w1",
            "kmux list --json",
            "kmux capabilities",
            "kmux open --dock right --cmd claude",
            "kmux open --dock md /tmp/a.md",
            "kmux open web a.test --dock=right --window new",
            "kmux move agent --dock",
            "kmux move agent --dock right --window w2",
            "kmux move agent --tab t3",
            "kmux move agent --tab --window w2",
            "kmux resize agent 1/3",
        ]
        .map(String::from),
    );
    for line in lines {
        let kmux = kmux_parse(&line).unwrap_or_else(|f| panic!("kmux: {line}: {f:?}"));
        let kanna = kanna_parse(&line).unwrap_or_else(|f| panic!("kanna: {line}: {f:?}"));
        assert_eq!(kanna, kmux, "{line}");
    }
}

#[test]
fn usage_errors_are_kmux_errors() {
    for line in [
        "kmux close",
        "kmux zoom",
        "kmux open top",
        "kmux open web",
        "kmux move logs",
        "kmux zoom a b",
        "kmux open --history",
        "kmux open md",
        "kmux open notes.md",
        "kmux open ios",
        "kmux open --app x",
        "kmux open --device x web u",
        "kmux navigate site --back x",
        "kmux navigate site --back --forward",
        "kmux navigate site",
        "kmux send p1",
        "kmux resize p1",
        "kmux arrange",
        "kmux arrange '{nope'",
        "kmux rename-tab",
        "kmux rename-tab t1",
        "kmux move-tab",
        "kmux move-tab t1 --index x",
        "kmux open --name",
        "kmux list extra",
        "kmux capabilities extra",
        "kmux open --dock --split right",
        "kmux open --dock left --tab",
        "kmux move agent --dock --tab",
        "kmux move agent --dock --to b",
        "kmux move agent --dock top",
    ] {
        let kmux = kmux_parse(line).expect_err(line);
        let kanna = kanna_parse(line).expect_err(line);
        assert_eq!(kanna.0, kmux.0, "{line}");
        assert_eq!(from_kanna(&kanna.1), from_kmux(&kmux.1), "{line}");
    }
}

#[test]
fn replies_print_as_kmux_prints_them() {
    let pane = json!({ "id": "p3", "name": "logs", "type": "term", "state": "running", "cmd": "tail -f a.log" });
    let layout = json!({ "split": "row", "children": [{ "pane": "p1", "size": "2/3" }, { "pane": "logs", "size": "1/3" }] });
    let list = json!({
        "windows": [
            { "id": "w1", "key": true, "focused": "logs", "zoomed": "logs", "tabs": [
                { "id": "t1", "title": "Tab 1", "active": true, "layout": layout },
                { "id": "t2", "title": "web", "active": false, "layout": { "pane": "site" } }
            ] },
            { "id": "w2", "key": false, "dock": { "pane": "doc", "edge": "right", "width": "1/3" },
              "tabs": [{ "id": "t3", "title": "Tab 1", "active": true, "layout": { "pane": "p9" } }] },
            { "id": "w3", "key": false, "focused": "phone", "dock": { "pane": "phone", "edge": "left", "width": "2/5" }, "tabs": [] }
        ],
        "panes": [
            { "id": "p1", "type": "term", "state": "running" },
            pane,
            { "id": "p4", "name": "done", "type": "term", "state": "exited", "exitCode": 3, "cmd": "exit 3" },
            { "id": "site", "type": "web", "state": "failed", "error": "a very long error that goes on and on and on and on and on and on and on\nsecond line", "url": "http://localhost:3000" },
            { "id": "p6", "name": "phone", "type": "ios", "state": "starting", "app": "com.apple.Preferences", "device": "iPhone 16" },
            { "id": "p7", "name": "doc", "type": "md", "state": "running", "path": "/tmp/a.md" }
        ]
    });
    let cases: Vec<(&str, Value, Value)> = vec![
        ("open", json!({ "type": "term" }), json!({ "pane": pane, "window": "w1", "tab": "t1" })),
        ("open", json!({ "type": "term" }), json!({ "pane": { "id": "p5", "state": "failed", "error": "No such file" }, "window": "w1", "tab": "t1" })),
        ("close", json!({ "pane": "logs" }), json!({ "closed": ["p3"] })),
        ("close", json!({ "tab": "t2" }), json!({ "closed": ["p3", "p4"] })),
        ("close", json!({ "window": "w2" }), json!({ "closed": ["p5"] })),
        ("focus", json!({ "pane": "logs" }), json!({ "window": "w1" })),
        ("focus", json!({ "tab": "t2" }), json!({ "window": "w1" })),
        ("focus", json!({ "window": "w2" }), json!({ "window": "w2" })),
        ("zoom", json!({ "pane": "logs" }), json!({ "zoomed": true })),
        ("zoom", json!({ "pane": "logs" }), json!({ "zoomed": false })),
        ("restart", json!({ "pane": "logs" }), json!({ "pane": pane })),
        ("send", json!({ "pane": "logs", "text": "x" }), json!({})),
        ("resize", json!({ "pane": "logs", "size": "1/3" }), json!({ "layout": layout })),
        ("move", json!({ "pane": "logs", "to": "p1" }), json!({ "layout": layout })),
        ("arrange", json!({ "layout": layout }), json!({ "layout": layout })),
        ("rename-tab", json!({ "tab": "t1", "title": "servers" }), json!({ "tab": "t1", "title": "servers" })),
        ("move-tab", json!({ "tab": "t2", "index": 0 }), json!({ "window": "w1", "tabs": ["t2", "t1"] })),
        ("navigate", json!({ "pane": "site", "url": "x" }), json!({ "pane": { "id": "p2", "name": "site", "state": "running", "url": "http://x/" } })),
        ("navigate", json!({ "pane": "doc", "path": "/tmp/a.md" }), json!({ "pane": { "id": "p7", "name": "doc", "state": "running", "path": "/tmp/a.md" } })),
        ("open", json!({ "type": "term", "dock": "left" }), json!({ "pane": pane, "window": "w1", "tab": null, "dock": "left" })),
        ("move", json!({ "pane": "logs", "dock": "right" }), json!({ "window": "w1", "dock": { "pane": "logs", "edge": "right", "width": "2/5" } })),
        ("resize", json!({ "pane": "logs", "size": "1/4" }), json!({ "window": "w1", "dock": { "pane": "logs", "edge": "left", "width": "1/4" } })),
        ("list", json!({}), list),
        ("list", json!({}), json!({ "windows": [], "panes": [] })),
        ("capabilities", json!({}), json!({ "mux": "kmux", "paneTypes": ["term", "web"], "commands": ["capabilities", "list"] })),
    ];
    for (name, args, reply) in cases {
        let kmux = (commands::find(name).unwrap().show)(&args, &reply);
        let kanna = (kanna::commands::find(name).unwrap().show)(&args, &reply);
        assert_eq!(from_kanna(&kanna), from_kmux(&kmux), "{name} {reply}");
    }
}

#[test]
fn kmux_cannot_type_what_kanna_can_after_a_double_dash() {
    let kanna = kanna_parse("kanna send p1 -- --json --mux").unwrap();
    assert_eq!(kanna, json!({ "pane": "p1", "text": "--json --mux" }));
}
