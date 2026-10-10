use crate::version::Range;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};

pub const SCHEMA: u32 = 1;
pub const ENGINE: u32 = 1;
pub const DEFAULT_LINES: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Status {
    Working,
    NeedsYou,
    Idle,
}

impl Status {
    pub const ALL: [Status; 3] = [Status::Working, Status::NeedsYou, Status::Idle];

    pub fn as_str(self) -> &'static str {
        match self {
            Status::Working => "working",
            Status::NeedsYou => "needs-you",
            Status::Idle => "idle",
        }
    }

    pub fn parse(name: &str) -> Option<Status> {
        Status::ALL.into_iter().find(|s| s.as_str() == name)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Screen {
    pub lines: Vec<String>,
    pub title: String,
    pub progress: String,
}

impl Screen {
    pub fn new(text: &str) -> Screen {
        Screen { lines: text.lines().map(str::to_owned).collect(), ..Screen::default() }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Screen {
        self.title = title.into();
        self
    }

    pub fn with_progress(mut self, progress: impl Into<String>) -> Screen {
        self.progress = progress.into();
        self
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleFile {
    pub schema: u32,
    pub agent: String,
    pub version: String,
    pub engine: u32,
    #[serde(default)]
    pub programs: Vec<String>,
    #[serde(default)]
    pub version_probe: Option<VersionProbe>,
    #[serde(default)]
    pub lines: Option<usize>,
    #[serde(default)]
    pub vocabulary: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionProbe {
    pub args: Vec<String>,
    pub pattern: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "any_version")]
    pub versions: String,
    pub when: Value,
}

fn any_version() -> String {
    "*".into()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Bundled,
    Remote(String),
    Override(String),
}

impl Source {
    pub fn label(&self) -> String {
        match self {
            Source::Bundled => "bundled".into(),
            Source::Remote(url) => format!("remote {url}"),
            Source::Override(path) => format!("override {path}"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Agent {
    pub file: RuleFile,
    pub source: Source,
    rules: Vec<(Rule, Status, Range)>,
    regexes: HashMap<String, Regex>,
}

impl Agent {
    pub fn parse(text: &str, source: Source) -> Result<Agent, String> {
        let file: RuleFile = serde_json::from_str(text).map_err(|e| format!("not a rule file: {e}"))?;
        Agent::compile(file, source)
    }

    pub fn compile(file: RuleFile, source: Source) -> Result<Agent, String> {
        if file.schema != SCHEMA {
            return Err(format!("schema {} isn't supported (this Kanna reads schema {SCHEMA})", file.schema));
        }
        if file.engine > ENGINE {
            return Err(format!("needs rule engine {} (this Kanna has {ENGINE})", file.engine));
        }
        if !safe_name(&file.agent) || file.version.trim().is_empty() {
            return Err("agent (letters, digits, - and _) and version are required".into());
        }
        if file.programs.iter().any(|p| p.trim().is_empty() || p.contains('/')) {
            return Err("programs are plain executable names".into());
        }
        if file.lines == Some(0) {
            return Err("lines must be above 0".into());
        }
        let mut regexes = HashMap::new();
        if let Some(probe) = &file.version_probe {
            regex(&mut regexes, &probe.pattern).map_err(|e| format!("versionProbe.pattern: {e}"))?;
        }
        let mut ids = HashSet::new();
        let mut rules = vec![];
        for rule in &file.rules {
            if rule.id.trim().is_empty() || !ids.insert(rule.id.clone()) {
                return Err(format!("rule ids must be non-empty and unique: \"{}\"", rule.id));
            }
            let status = Status::parse(&rule.status).ok_or_else(|| format!("{}: unknown status \"{}\" (working, needs-you or idle)", rule.id, rule.status))?;
            let range = Range::parse(&rule.versions).map_err(|e| format!("{}: {e}", rule.id))?;
            check_when(&rule.when, &file, &mut regexes).map_err(|e| format!("{}: {e}", rule.id))?;
            rules.push((rule.clone(), status, range));
        }
        rules.sort_by_key(|(rule, _, _)| std::cmp::Reverse(rule.priority));
        Ok(Agent { file, source, rules, regexes })
    }

    pub fn name(&self) -> &str {
        &self.file.agent
    }

    pub fn programs(&self) -> Vec<String> {
        if self.file.programs.is_empty() {
            vec![self.file.agent.clone()]
        } else {
            self.file.programs.clone()
        }
    }

    pub fn probe_pattern(&self) -> Option<&Regex> {
        self.file.version_probe.as_ref().and_then(|p| self.regexes.get(&p.pattern))
    }

    pub fn evaluate(&self, screen: &Screen, version: Option<&str>) -> Verdict {
        let excluded = self.rules.iter().filter(|(_, _, range)| !range.admits(version)).map(|(rule, _, _)| rule.id.clone()).collect();
        let mut verdict = Verdict {
            agent: self.file.agent.clone(),
            status: Status::Idle,
            rule: None,
            matched: None,
            file_version: self.file.version.clone(),
            source: self.source.label(),
            agent_version: version.map(str::to_owned),
            excluded,
        };
        for (rule, status, range) in &self.rules {
            if !range.admits(version) {
                continue;
            }
            let eval = Eval { agent: self, screen, last: self.file.lines.unwrap_or(DEFAULT_LINES), matched: RefCell::new(None), negated: RefCell::new(0) };
            if eval.when(&rule.when, None) {
                verdict.status = *status;
                verdict.rule = Some(rule.id.clone());
                verdict.matched = eval.matched.into_inner();
                return verdict;
            }
        }
        verdict
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub agent: String,
    pub status: Status,
    pub rule: Option<String>,
    pub matched: Option<String>,
    pub file_version: String,
    pub source: String,
    pub agent_version: Option<String>,
    pub excluded: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct RuleSet {
    agents: BTreeMap<String, Agent>,
}

impl RuleSet {
    pub fn insert(&mut self, agent: Agent) {
        self.agents.insert(agent.file.agent.clone(), agent);
    }

    pub fn agent(&self, name: &str) -> Option<&Agent> {
        self.agents.get(name)
    }

    pub fn agents(&self) -> impl Iterator<Item = &Agent> {
        self.agents.values()
    }

    pub fn evaluate(&self, agent: &str, screen: &Screen, version: Option<&str>) -> Option<Verdict> {
        self.agents.get(agent).map(|a| a.evaluate(screen, version))
    }

    pub fn agent_for_program(&self, program: &str) -> Option<&Agent> {
        self.agents.values().find(|a| a.programs().iter().any(|p| p == program))
    }
}

pub fn safe_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn regex(regexes: &mut HashMap<String, Regex>, pattern: &str) -> Result<(), String> {
    if !regexes.contains_key(pattern) {
        let compiled = Regex::new(pattern).map_err(|e| e.to_string())?;
        regexes.insert(pattern.into(), compiled);
    }
    Ok(())
}

fn object(value: &Value) -> Result<(&String, &Value), String> {
    let map = value.as_object().ok_or("a test must be an object")?;
    if map.len() != 1 {
        let keys: Vec<&str> = map.keys().map(String::as_str).collect();
        return Err(format!("a test has exactly one key, not {keys:?}"));
    }
    Ok(map.iter().next().unwrap())
}

fn check_when(value: &Value, file: &RuleFile, regexes: &mut HashMap<String, Regex>) -> Result<(), String> {
    if let Some(map) = value.as_object() {
        if map.len() == 2 && map.contains_key("region") && map.contains_key("when") {
            check_region(&map["region"], file, regexes)?;
            return check_when(&map["when"], file, regexes);
        }
    }
    let (key, arg) = object(value)?;
    match key.as_str() {
        "all" | "any" => {
            let items = arg.as_array().ok_or(format!("{key} takes a list"))?;
            items.iter().try_for_each(|item| check_when(item, file, regexes))
        }
        "not" => check_when(arg, file, regexes),
        "anyLine" | "anyLineWrapped" | "lastNonEmptyLine" | "title" | "progress" => check_text(arg, file, regexes),
        "region" => Err("region goes with when: {\"region\": …, \"when\": …}".into()),
        _ => Err(format!("unknown test \"{key}\"")),
    }
}

fn check_region(value: &Value, file: &RuleFile, regexes: &mut HashMap<String, Regex>) -> Result<(), String> {
    if value == "screen" {
        return Ok(());
    }
    let (key, arg) = object(value)?;
    match key.as_str() {
        "lastLines" | "lastNonEmptyLines" | "firstNonEmptyLines" => arg.as_u64().map(|_| ()).ok_or(format!("{key} takes a line count")),
        "linesAbove" | "linesBelow" => {
            arg.get("n").and_then(Value::as_u64).ok_or(format!("{key} needs n"))?;
            check_text(arg.get("anchor").ok_or(format!("{key} needs an anchor"))?, file, regexes)
        }
        "between" => {
            check_text(arg.get("from").ok_or("between needs from")?, file, regexes)?;
            check_text(arg.get("to").ok_or("between needs to")?, file, regexes)
        }
        _ => Err(format!("unknown region \"{key}\"")),
    }
}

fn check_text(value: &Value, file: &RuleFile, regexes: &mut HashMap<String, Regex>) -> Result<(), String> {
    let (key, arg) = object(value)?;
    match key.as_str() {
        "allOf" | "anyOf" => {
            let items = arg.as_array().ok_or(format!("{key} takes a list"))?;
            items.iter().try_for_each(|item| check_text(item, file, regexes))
        }
        "not" => check_text(arg, file, regexes),
        "contains" | "startsWith" | "equals" => arg.as_str().map(|_| ()).ok_or(format!("{key} takes text")),
        "containsAll" => match arg.as_array() {
            Some(items) if items.iter().all(Value::is_string) => Ok(()),
            _ => Err("containsAll takes a list of text".into()),
        },
        "containsAny" | "containsAllOf" | "startsWithAny" | "startsWithGlyph" | "startsWithGlyphWord" | "glyphOnly" | "allCharactersIn" => {
            let name = arg.as_str().ok_or(format!("{key} takes a vocabulary name"))?;
            file.vocabulary.contains_key(name).then_some(()).ok_or(format!("no vocabulary \"{name}\""))
        }
        "regex" => regex(regexes, arg.as_str().ok_or("regex takes a pattern")?),
        _ => Err(format!("unknown text test \"{key}\"")),
    }
}

struct Eval<'a> {
    agent: &'a Agent,
    screen: &'a Screen,
    last: usize,
    matched: RefCell<Option<String>>,
    negated: RefCell<usize>,
}

impl Eval<'_> {
    fn default_rows(&self) -> &[String] {
        let lines = &self.screen.lines;
        &lines[lines.len().saturating_sub(self.last)..]
    }

    fn found(&self, text: &str) -> bool {
        if *self.negated.borrow() == 0 && self.matched.borrow().is_none() {
            *self.matched.borrow_mut() = Some(text.to_owned());
        }
        true
    }

    fn negate<T>(&self, f: impl FnOnce() -> T) -> T {
        *self.negated.borrow_mut() += 1;
        let out = f();
        *self.negated.borrow_mut() -= 1;
        out
    }

    fn when(&self, value: &Value, rows: Option<&[String]>) -> bool {
        let Some(map) = value.as_object() else { return false };
        if let (Some(region), Some(inner)) = (map.get("region"), map.get("when")) {
            let base = rows.unwrap_or(&self.screen.lines);
            let selected = self.select(region, base);
            return self.when(inner, Some(&selected));
        }
        let Some((key, arg)) = map.iter().next() else { return false };
        let rows = rows.unwrap_or_else(|| self.default_rows());
        match key.as_str() {
            "all" => arg.as_array().is_some_and(|items| items.iter().all(|item| self.when(item, Some(rows)))),
            "any" => arg.as_array().is_some_and(|items| items.iter().any(|item| self.when(item, Some(rows)))),
            "not" => !self.negate(|| self.when(arg, Some(rows))),
            "anyLine" => rows.iter().any(|line| self.text(arg, line) && self.found(line)),
            "anyLineWrapped" => {
                rows.iter().any(|line| self.text(arg, line) && self.found(line))
                    || rows.windows(2).map(|pair| format!("{} {}", pair[0], pair[1])).any(|joined| self.text(arg, &joined) && self.found(&joined))
            }
            "lastNonEmptyLine" => rows.iter().rev().find(|line| !line.trim().is_empty()).is_some_and(|line| self.text(arg, line) && self.found(line)),
            "title" => self.text(arg, &self.screen.title) && self.found(&self.screen.title),
            "progress" => self.text(arg, &self.screen.progress) && self.found(&format!("progress {}", self.screen.progress)),
            _ => false,
        }
    }

    fn select(&self, region: &Value, rows: &[String]) -> Vec<String> {
        if region == "screen" {
            return self.screen.lines.clone();
        }
        let Some((kind, arg)) = region.as_object().and_then(|m| m.iter().next()) else { return rows.to_vec() };
        let count = |v: &Value| v.as_u64().unwrap_or(0) as usize;
        match kind.as_str() {
            "lastLines" => rows[rows.len().saturating_sub(count(arg))..].to_vec(),
            "lastNonEmptyLines" => {
                let n = count(arg);
                let start = rows.iter().enumerate().rev().filter(|(_, l)| !l.trim().is_empty()).take(n).last().map(|(i, _)| i).unwrap_or(rows.len());
                rows[start..].to_vec()
            }
            "firstNonEmptyLines" => {
                let n = count(arg);
                let end = rows.iter().enumerate().filter(|(_, l)| !l.trim().is_empty()).take(n).last().map(|(i, _)| i + 1).unwrap_or(0);
                rows[..end].to_vec()
            }
            "linesAbove" | "linesBelow" => {
                let Some(anchor) = arg.get("anchor") else { return vec![] };
                let Some(at) = rows.iter().rposition(|line| self.text(anchor, line)) else { return vec![] };
                let n = arg.get("n").map(count).unwrap_or(rows.len());
                if kind == "linesAbove" {
                    rows[at.saturating_sub(n)..at].to_vec()
                } else {
                    rows[at + 1..(at + 1 + n).min(rows.len())].to_vec()
                }
            }
            "between" => {
                let (Some(from), Some(to)) = (arg.get("from"), arg.get("to")) else { return vec![] };
                let Some(start) = rows.iter().rposition(|line| self.text(from, line)) else { return vec![] };
                let Some(end) = rows[start + 1..].iter().position(|line| self.text(to, line)).map(|j| start + 1 + j) else { return vec![] };
                rows[start + 1..end].to_vec()
            }
            _ => rows.to_vec(),
        }
    }

    fn vocabulary(&self, name: &Value) -> &[String] {
        name.as_str().and_then(|n| self.agent.file.vocabulary.get(n)).map(Vec::as_slice).unwrap_or(&[])
    }

    fn text(&self, value: &Value, text: &str) -> bool {
        let Some((key, arg)) = value.as_object().and_then(|m| m.iter().next()) else { return false };
        let lower = || text.to_lowercase();
        let has = |needle: &str| lower().contains(&needle.to_lowercase());
        match key.as_str() {
            "allOf" => arg.as_array().is_some_and(|items| items.iter().all(|item| self.text(item, text))),
            "anyOf" => arg.as_array().is_some_and(|items| items.iter().any(|item| self.text(item, text))),
            "not" => !self.text(arg, text),
            "contains" => arg.as_str().is_some_and(has),
            "containsAll" => arg.as_array().is_some_and(|items| items.iter().all(|n| n.as_str().is_some_and(has))),
            "containsAny" => self.vocabulary(arg).iter().any(|n| has(n)),
            "containsAllOf" => {
                let words = self.vocabulary(arg);
                !words.is_empty() && words.iter().all(|n| has(n))
            }
            "startsWith" => arg.as_str().is_some_and(|s| text.trim_start().starts_with(s)),
            "startsWithAny" => self.vocabulary(arg).iter().any(|s| text.trim_start().starts_with(s.as_str())),
            "startsWithGlyph" => starts_with_glyph(text, self.vocabulary(arg), false),
            "startsWithGlyphWord" => starts_with_glyph(text, self.vocabulary(arg), true),
            "glyphOnly" => self.vocabulary(arg).iter().any(|g| text.trim() == g),
            "allCharactersIn" => {
                let glyphs = self.vocabulary(arg);
                let t = text.trim();
                !t.is_empty() && t.chars().all(|c| c == ' ' || glyphs.iter().any(|g| g.chars().count() == 1 && g.starts_with(c)))
            }
            "equals" => arg.as_str().is_some_and(|s| text.trim() == s),
            "regex" => arg.as_str().and_then(|p| self.agent.regexes.get(p)).is_some_and(|r| r.is_match(text)),
            _ => false,
        }
    }
}

fn starts_with_glyph(text: &str, glyphs: &[String], word: bool) -> bool {
    let mut chars = text.trim_start().chars();
    let Some(first) = chars.next() else { return false };
    glyphs.iter().any(|g| g.starts_with(first)) && (!word || chars.next().is_some_and(char::is_whitespace))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(rules: &str) -> Agent {
        let text = format!(r#"{{"schema":1,"agent":"test","version":"1","engine":1,"vocabulary":{{"prompt":["›"]}},"rules":{rules}}}"#);
        Agent::parse(&text, Source::Bundled).unwrap()
    }

    #[test]
    fn the_highest_priority_match_wins_and_no_match_is_idle() {
        let a = agent(
            r#"[{"id":"low","status":"working","priority":1,"when":{"anyLine":{"contains":"marker"}}},
                {"id":"high","status":"needs-you","priority":9,"when":{"anyLine":{"contains":"proceed?"}}}]"#,
        );
        let v = a.evaluate(&Screen::new("marker\nDo you want to proceed?"), None);
        assert_eq!((v.status, v.rule.as_deref(), v.matched.as_deref()), (Status::NeedsYou, Some("high"), Some("Do you want to proceed?")));
        let v = a.evaluate(&Screen::new("nothing here"), None);
        assert_eq!((v.status, v.rule), (Status::Idle, None));
    }

    #[test]
    fn tests_read_the_last_ten_lines_unless_they_name_a_region() {
        let a = agent(
            r#"[{"id":"bottom","status":"needs-you","priority":2,"when":{"anyLine":{"contains":"proceed?"}}},
                {"id":"anywhere","status":"working","priority":1,"when":{"region":"screen","when":{"anyLine":{"contains":"proceed?"}}}}]"#,
        );
        let mut lines = vec!["Do you want to proceed?".to_string()];
        lines.extend((0..10).map(|i| format!("line {i}")));
        let v = a.evaluate(&Screen { lines, ..Screen::default() }, None);
        assert_eq!(v.rule.as_deref(), Some("anywhere"));
    }

    #[test]
    fn unknown_versions_only_use_star_rules() {
        let a = agent(
            r#"[{"id":"versioned","status":"working","priority":9,"versions":">=2.0","when":{"anyLine":{"contains":"marker"}}},
                {"id":"fallback","status":"idle","priority":1,"when":{"anyLine":{"contains":"marker"}}}]"#,
        );
        let v = a.evaluate(&Screen::new("marker"), None);
        assert_eq!(v.rule.as_deref(), Some("fallback"));
        assert_eq!(v.excluded, ["versioned"]);
        assert_eq!(a.evaluate(&Screen::new("marker"), Some("2.1.0")).rule.as_deref(), Some("versioned"));
    }

    #[test]
    fn title_progress_and_regions_are_tested() {
        let a = agent(
            r#"[{"id":"progress","status":"working","priority":3,"when":{"progress":{"equals":"indeterminate"}}},
                {"id":"title","status":"working","priority":2,"when":{"title":{"startsWith":"⠋"}}},
                {"id":"composer","status":"idle","priority":1,"when":{"region":{"linesBelow":{"anchor":{"startsWith":"───"},"n":2}},"when":{"anyLine":{"startsWithGlyph":"prompt"}}}}]"#,
        );
        assert_eq!(a.evaluate(&Screen::new("x").with_progress("indeterminate"), None).rule.as_deref(), Some("progress"));
        assert_eq!(a.evaluate(&Screen::new("x").with_title("⠋ ⠋ | work"), None).rule.as_deref(), Some("title"));
        assert_eq!(a.evaluate(&Screen::new("› old\n───\n› "), None).rule.as_deref(), Some("composer"));
        assert_eq!(a.evaluate(&Screen::new("› old\n───\ntext"), None).rule, None);
    }

    #[test]
    fn negated_matches_are_not_reported() {
        let a = agent(r#"[{"id":"r","status":"idle","when":{"all":[{"not":{"anyLine":{"contains":"busy"}}},{"anyLine":{"startsWithGlyph":"prompt"}}]}}]"#);
        assert_eq!(a.evaluate(&Screen::new("› hi"), None).matched.as_deref(), Some("› hi"));
    }

    #[test]
    fn bad_rule_files_are_refused() {
        let bad = |rules: &str| {
            let text = format!(r#"{{"schema":1,"agent":"test","version":"1","engine":1,"vocabulary":{{}},"rules":{rules}}}"#);
            Agent::parse(&text, Source::Bundled).is_err()
        };
        assert!(bad(r#"[{"id":"a","status":"busy","when":{"anyLine":{"contains":"x"}}}]"#));
        assert!(bad(r#"[{"id":"a","status":"idle","when":{"anyLine":{"containsAny":"missing"}}}]"#));
        assert!(bad(r#"[{"id":"a","status":"idle","when":{"anyLine":{"regex":"("}}}]"#));
        assert!(bad(r#"[{"id":"a","status":"idle","when":{"someLine":{"contains":"x"}}}]"#));
        assert!(bad(r#"[{"id":"a","status":"idle","when":{"anyLine":{"contains":"x"}}},{"id":"a","status":"idle","when":{"anyLine":{"contains":"y"}}}]"#));
        assert!(bad(r#"[{"id":"a","status":"idle","versions":"~1","when":{"anyLine":{"contains":"x"}}}]"#));
        let future = r#"{"schema":1,"agent":"test","version":"1","engine":2,"rules":[]}"#;
        assert!(Agent::parse(future, Source::Bundled).is_err());
    }
}
