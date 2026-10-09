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
    format!(
        "opened {} in window {}, tab {}: {}",
        label(&reply["pane"]),
        reply["window"].as_str().unwrap_or("?"),
        reply["tab"].as_str().unwrap_or("?"),
        state(&reply["pane"])
    )
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
    let mut rows = vec![["PANE".to_string(), "NAME".into(), "TYPE".into(), "STATE".into(), "COMMAND".into()]];
    for pane in panes {
        rows.push([
            pane["id"].as_str().unwrap_or("?").into(),
            pane["name"].as_str().unwrap_or("-").into(),
            pane["type"].as_str().unwrap_or("?").into(),
            brief(&state(pane)),
            what(pane).unwrap_or_else(|| "(shell)".into()),
        ]);
    }
    let widths: Vec<usize> = (0..5).map(|i| rows.iter().map(|r| r[i].chars().count()).max().unwrap_or(0)).collect();
    for row in rows {
        out.push(row.iter().zip(&widths).map(|(cell, w)| format!("{cell:w$}")).collect::<Vec<_>>().join("  ").trim_end().to_string());
    }
    out.join("\n")
}
