//! Human-readable output for replies.

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

pub fn opened(_: &Value, reply: &Value) -> String {
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

pub fn pane_state(reply: &Value) -> String {
    let pane = &reply["pane"];
    let what = pane["cmd"].as_str().or(pane["url"].as_str()).map(|c| format!(" ({c})")).unwrap_or_default();
    format!("{}{what} is {}", label(pane), state(pane))
}

pub fn layout(reply: &Value) -> String {
    expression(&reply["layout"], true)
}

/// A layout tree as a layout expression: `a:2/3 | b`, `a / b`, with
/// parentheses for nested splits (the kanna layout syntax).
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
    if windows.is_empty() {
        return "no windows (open one with `kmux open`)".into();
    }
    let mut out = Vec::new();
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
    for pane in reply["panes"].as_array().unwrap_or(&empty) {
        rows.push([
            pane["id"].as_str().unwrap_or("?").into(),
            pane["name"].as_str().unwrap_or("-").into(),
            pane["type"].as_str().unwrap_or("?").into(),
            state(pane),
            pane["cmd"].as_str().or(pane["url"].as_str()).unwrap_or("(shell)").into(),
        ]);
    }
    let widths: Vec<usize> = (0..5).map(|i| rows.iter().map(|r| r[i].chars().count()).max().unwrap_or(0)).collect();
    for row in rows {
        out.push(row.iter().zip(&widths).map(|(cell, w)| format!("{cell:w$}")).collect::<Vec<_>>().join("  ").trim_end().to_string());
    }
    out.join("\n")
}
