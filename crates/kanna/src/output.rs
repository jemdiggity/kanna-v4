use serde_json::Value;

pub fn join(list: &Value) -> String {
    list.as_array().map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default()
}

fn label(pane: &Value) -> String {
    let id = pane["id"].as_str().unwrap_or("?");
    match pane["name"].as_str() {
        Some(name) => format!("{id} ({name})"),
        None => id.to_string(),
    }
}

fn state(pane: &Value) -> String {
    let state = pane["state"].as_str().unwrap_or("?");
    match (pane["exitCode"].as_i64(), pane["error"].as_str()) {
        (Some(code), _) => format!("{state} ({code})"),
        (_, Some(error)) => format!("{state}: {error}"),
        _ => state.to_string(),
    }
}

fn brief(text: &str) -> String {
    let line = text.lines().next().unwrap_or("");
    if line.chars().count() > 60 { format!("{}…", line.chars().take(59).collect::<String>()) } else { line.to_string() }
}

pub fn opened(_: &Value, reply: &Value) -> String {
    if reply["window"].is_null() && reply.get("window").is_some() {
        return format!("opened {}, unplaced until Kanna.app opens: {}", label(&reply["pane"]), state(&reply["pane"]));
    }
    let window = reply["window"].as_str().unwrap_or("?");
    if let Some(edge) = reply["dock"].as_str() {
        return format!("opened {} docked {edge} in window {window}: {}", label(&reply["pane"]), state(&reply["pane"]));
    }
    format!("opened {} in window {window}, tab {}: {}", label(&reply["pane"]), reply["tab"].as_str().unwrap_or("?"), state(&reply["pane"]))
}

pub fn marked(args: &Value, reply: &Value) -> String {
    if args.get("type").is_some() {
        return opened(args, reply);
    }
    let pane = &reply["pane"];
    let to = pane["agent"].as_str().map(|agent| format!(", sent to {agent}")).unwrap_or_default();
    match pane["markup"] == true {
        true => format!("comments on in {}{to}", label(pane)),
        false => format!("comments off in {}", label(pane)),
    }
}

pub fn moved(reply: &Value) -> String {
    let dock = &reply["dock"];
    if dock.is_object() {
        return format!(
            "{} is docked {} in {} (width {})",
            dock["pane"].as_str().unwrap_or("?"),
            dock["edge"].as_str().unwrap_or("?"),
            reply["window"].as_str().unwrap_or("?"),
            dock["width"].as_str().unwrap_or("?")
        );
    }
    layout(reply)
}

pub fn closed(args: &Value, reply: &Value) -> String {
    let panes = join(&reply["closed"]);
    match (args["window"].as_str(), args["tab"].as_str()) {
        (Some(window), _) => format!("closed window {window} (panes {panes})"),
        (_, Some(tab)) => format!("closed tab {tab} (panes {panes})"),
        _ => format!("closed {panes}"),
    }
}

pub fn focused(args: &Value, reply: &Value) -> String {
    match (args["pane"].as_str(), args["tab"].as_str()) {
        (Some(pane), _) => format!("focused {pane} (key window {})", reply["window"].as_str().unwrap_or("?")),
        (_, Some(tab)) => format!("switched to {tab} (key window {})", reply["window"].as_str().unwrap_or("?")),
        _ => format!("key window is now {}", reply["window"].as_str().unwrap_or("?")),
    }
}

pub fn zoomed(args: &Value, reply: &Value) -> String {
    let pane = args["pane"].as_str().unwrap_or("pane");
    if reply["zoomed"] == serde_json::json!(true) { format!("{pane} is zoomed (run again to unzoom)") } else { format!("{pane} is no longer zoomed") }
}

pub fn renamed(_: &Value, reply: &Value) -> String {
    format!("{} is now \"{}\"", reply["tab"].as_str().unwrap_or("?"), reply["title"].as_str().unwrap_or(""))
}

pub fn pane_renamed(_: &Value, reply: &Value) -> String {
    let pane = &reply["pane"];
    let id = pane["id"].as_str().unwrap_or("?");
    match pane["name"].as_str() {
        Some(name) => format!("{id} is now \"{name}\""),
        None => format!("{id} has no name"),
    }
}

pub fn tabs(_: &Value, reply: &Value) -> String {
    format!("{}: {}", reply["window"].as_str().unwrap_or("?"), join(&reply["tabs"]))
}

pub fn capabilities(reply: &Value) -> String {
    let mut out = format!("pane types: {}\ncommands: {}", join(&reply["paneTypes"]), join(&reply["commands"]));
    if let Some(app) = reply["app"].as_bool() {
        out += if app {
            "\nKanna.app: attached"
        } else {
            "\nKanna.app: not attached (terminals work without it; the CLI starts it for commands that need it)"
        };
    }
    out
}

fn what(pane: &Value) -> Option<String> {
    if let Some(app) = pane["app"].as_str() {
        return Some(match pane["device"].as_str() {
            Some(device) => format!("{app} on {device}"),
            None => app.into(),
        });
    }
    if let Some(path) = pane["path"].as_str() {
        return Some(match std::env::var("HOME") {
            Ok(home) if !home.is_empty() && path.starts_with(&format!("{home}/")) => format!("~{}", &path[home.len()..]),
            _ => path.into(),
        });
    }
    pane["cmd"].as_str().or(pane["url"].as_str()).map(String::from)
}

pub fn pane_state(reply: &Value) -> String {
    let pane = &reply["pane"];
    let what = what(pane).map(|c| format!(" ({c})")).unwrap_or_default();
    format!("{}{what} is {}", label(pane), state(pane))
}

pub fn layout(reply: &Value) -> String {
    expression(&reply["layout"], true)
}

pub fn expression(node: &Value, top: bool) -> String {
    if let Some(pane) = node["pane"].as_str() {
        return sized(pane.to_string(), node);
    }
    let separator = if node["split"] == "column" { " / " } else { " | " };
    let inner = node["children"].as_array().map(|kids| kids.iter().map(|k| expression(k, false)).collect::<Vec<_>>().join(separator)).unwrap_or_default();
    if top { inner } else { sized(format!("({inner})"), node) }
}

fn sized(text: String, node: &Value) -> String {
    match node["size"].as_str() {
        Some(size) => format!("{text}:{size}"),
        None => text,
    }
}

pub fn list(reply: &Value) -> String {
    let empty = vec![];
    let windows = reply["windows"].as_array().unwrap_or(&empty);
    let panes = reply["panes"].as_array().unwrap_or(&empty);
    if windows.is_empty() && panes.is_empty() {
        return "no windows (open one with `kanna open`)".into();
    }
    let mut out = Vec::new();
    if windows.is_empty() {
        out.push("no windows: Kanna.app isn't open (these terminals keep running)".to_string());
    }
    for window in windows {
        let mut line = format!("{}{}", if window["key"] == true { "*" } else { " " }, window["id"].as_str().unwrap_or("?"));
        if let Some(focused) = window["focused"].as_str() {
            line += &format!("  focused: {focused}");
        }
        if let Some(zoomed) = window["zoomed"].as_str() {
            line += &format!("  zoomed: {zoomed}");
        }
        out.push(line);
        let dock = &window["dock"];
        if let Some(name) = dock["pane"].as_str() {
            let pane = panes.iter().find(|p| p["name"] == name || p["id"] == name);
            out.push(format!(
                "  dock: {}  {} {}",
                pane.map(label).unwrap_or_else(|| name.to_string()),
                dock["edge"].as_str().unwrap_or("?"),
                dock["width"].as_str().unwrap_or("?")
            ));
        }
        for tab in window["tabs"].as_array().unwrap_or(&empty) {
            out.push(format!(
                "  {}{} \"{}\"  {}",
                if tab["active"] == true { "*" } else { " " },
                tab["id"].as_str().unwrap_or("?"),
                tab["title"].as_str().unwrap_or(""),
                expression(&tab["layout"], true)
            ));
        }
    }
    out.push(String::new());
    let statuses = panes.iter().any(|p| p["status"].is_string());
    let mut header = vec!["PANE".to_string(), "NAME".into(), "TYPE".into(), "STATE".into()];
    if statuses {
        header.push("STATUS".into());
    }
    header.push("COMMAND".into());
    let mut rows = vec![header];
    for pane in panes {
        let mut row: Vec<String> = vec![
            pane["id"].as_str().unwrap_or("?").into(),
            pane["name"].as_str().unwrap_or("-").into(),
            pane["type"].as_str().unwrap_or("?").into(),
            brief(&state(pane)),
        ];
        if statuses {
            row.push(pane["status"].as_str().unwrap_or("-").into());
        }
        row.push(what(pane).unwrap_or_else(|| "(shell)".into()));
        rows.push(row);
    }
    let widths: Vec<usize> = (0..rows[0].len()).map(|i| rows.iter().map(|r| r[i].chars().count()).max().unwrap_or(0)).collect();
    for row in rows {
        out.push(row.iter().zip(&widths).map(|(cell, w)| format!("{cell:w$}")).collect::<Vec<_>>().join("  ").trim_end().to_string());
    }
    out.join("\n")
}

pub fn status(args: &Value, reply: &Value) -> String {
    if args["action"] == "update" {
        let mut out = vec![match reply["site"].as_str() {
            Some(site) => format!("checked {site}"),
            None => "no rules site is set".to_string(),
        }];
        let fetched = join(&reply["fetched"]);
        out.push(if fetched.is_empty() { "nothing new".into() } else { format!("fetched: {fetched}") });
        for problem in reply["problems"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            out.push(format!("problem: {problem}"));
        }
        for rules in reply["rules"].as_array().into_iter().flatten() {
            out.push(format!("{:<12} {:<14} {}", rules["agent"].as_str().unwrap_or("?"), rules["version"].as_str().unwrap_or("?"), rules["source"].as_str().unwrap_or("?")));
        }
        return out.join("\n");
    }
    let mut out = vec![];
    let who = reply["pane"].as_str().map(|p| format!("{p}: ")).unwrap_or_default();
    match reply["agent"].as_str() {
        None => return format!("{who}not an agent Kanna knows (its foreground program has no rules){}", reply["status"].as_str().map(|s| format!("; status {s}")).unwrap_or_default()),
        Some(agent) => {
            let version = reply["agentVersion"].as_str().map(|v| format!(" {v}")).unwrap_or_else(|| " (version unknown)".into());
            let exe = reply["executable"].as_str().map(|e| format!(" ({e})")).unwrap_or_default();
            out.push(format!("{who}{agent}{version}{exe}"));
        }
    }
    out.push(format!("rules: {} ({})", reply["rules"]["version"].as_str().unwrap_or("?"), reply["rules"]["source"].as_str().unwrap_or("?")));
    let judged = reply["judged"].as_str().unwrap_or("?");
    match reply["status"].as_str() {
        Some(status) if status != judged => out.push(format!("status: {status} (this screen judges {judged})")),
        _ => out.push(format!("status: {judged}")),
    }
    match reply["rule"].as_str() {
        Some(rule) => {
            out.push(format!("rule: {rule}"));
            if let Some(matched) = reply["matched"].as_str() {
                out.push(format!("matched: {}", matched.trim_end()));
            }
        }
        None => out.push("rule: none matched, so idle".into()),
    }
    let excluded = join(&reply["excluded"]);
    if !excluded.is_empty() {
        out.push(format!("not for this version: {excluded}"));
    }
    out.join("\n")
}

#[cfg(test)]
mod status_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn list_has_a_status_column_only_when_a_pane_has_a_status() {
        let plain = json!({ "windows": [], "panes": [{ "id": "p1", "name": null, "type": "term", "state": "running", "cmd": null }] });
        assert!(!list(&plain).contains("STATUS"));
        let agents = json!({ "windows": [], "panes": [
            { "id": "p1", "name": "a", "type": "term", "state": "running", "status": "needs-you", "cmd": "claude" },
            { "id": "p2", "name": null, "type": "term", "state": "running", "cmd": null }
        ] });
        let text = list(&agents);
        assert!(text.contains("PANE  NAME  TYPE  STATE    STATUS     COMMAND"), "{text}");
        assert!(text.contains("p1    a     term  running  needs-you  claude"), "{text}");
        assert!(text.contains("p2    -     term  running  -          (shell)"), "{text}");
    }

    #[test]
    fn explain_says_why() {
        let reply = json!({ "pane": "p3", "agent": "claude", "agentVersion": "2.1.296", "executable": "/bin/claude", "status": "working",
            "rules": { "version": "2026.10.10.1", "source": "bundled" }, "judged": "working", "rule": "claude/working/title", "matched": "◐ Claude Code", "excluded": [] });
        assert_eq!(
            status(&json!({ "action": "explain" }), &reply),
            "p3: claude 2.1.296 (/bin/claude)\nrules: 2026.10.10.1 (bundled)\nstatus: working\nrule: claude/working/title\nmatched: ◐ Claude Code"
        );
    }
}
