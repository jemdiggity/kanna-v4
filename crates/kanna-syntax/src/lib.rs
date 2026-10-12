mod ffi;
mod languages;

pub use languages::{language_for, Language, LANGUAGES};

use std::collections::HashMap;
use std::sync::OnceLock;
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

pub const CAPTURES: &[&str] = &[
    "attribute",
    "boolean",
    "character",
    "comment",
    "comment.documentation",
    "constant",
    "constant.builtin",
    "constant.macro",
    "constructor",
    "embedded",
    "escape",
    "function",
    "function.builtin",
    "function.call",
    "function.macro",
    "function.method",
    "keyword",
    "keyword.conditional",
    "keyword.coroutine",
    "keyword.directive",
    "keyword.exception",
    "keyword.function",
    "keyword.import",
    "keyword.modifier",
    "keyword.operator",
    "keyword.repeat",
    "keyword.return",
    "keyword.type",
    "label",
    "module",
    "number",
    "number.float",
    "operator",
    "property",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation.special",
    "string",
    "string.escape",
    "string.regexp",
    "string.special",
    "tag",
    "tag.attribute",
    "tag.delimiter",
    "type",
    "type.builtin",
    "type.definition",
    "variable",
    "variable.builtin",
    "variable.member",
    "variable.parameter",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub end: u32,
    pub capture: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scope {
    pub first: u32,
    pub last: u32,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Highlights {
    pub spans: Vec<Span>,
    pub scopes: Vec<Scope>,
}

struct Compiled {
    highlight: HighlightConfiguration,
    scopes: Option<Query>,
}

fn compiled(name: &str) -> Option<&'static Compiled> {
    static ALL: OnceLock<HashMap<&'static str, (&'static Language, OnceLock<Option<Compiled>>)>> = OnceLock::new();
    let all = ALL.get_or_init(|| LANGUAGES.iter().map(|language| (language.name, (language, OnceLock::new()))).collect());
    let (language, compiled) = all.get(name)?;
    compiled.get_or_init(|| compile(language)).as_ref()
}

fn compile(language: &Language) -> Option<Compiled> {
    let grammar = (language.grammar)();
    let join = |parts: &[&str]| {
        let joined = parts.join("\n");
        if language.neovim { languages::neovim(&joined) } else { joined }
    };
    let (highlights, injections, locals) = (join(language.highlights), join(language.injections), join(language.locals));
    let mut highlight = HighlightConfiguration::new(grammar.clone(), language.name, &highlights, &injections, &locals).ok()?;
    highlight.configure(CAPTURES);
    let scopes = (!language.scopes.is_empty()).then(|| Query::new(&grammar, language.scopes).ok()).flatten();
    Some(Compiled { highlight, scopes })
}

pub fn problems() -> Vec<String> {
    let mut out = Vec::new();
    for language in LANGUAGES {
        let grammar = (language.grammar)();
        let join = |parts: &[&str]| {
            let joined = parts.join("\n");
            if language.neovim { languages::neovim(&joined) } else { joined }
        };
        let (highlights, injections, locals) = (join(language.highlights), join(language.injections), join(language.locals));
        if let Err(e) = HighlightConfiguration::new(grammar.clone(), language.name, &highlights, &injections, &locals) {
            out.push(format!("{}: highlights: {e:?}", language.name));
        }
        if let Err(e) = Query::new(&grammar, language.scopes) {
            out.push(format!("{}: scopes: {e}", language.name));
        }
    }
    out
}

pub fn highlight(language: &str, text: &str) -> Option<Highlights> {
    let compiled = compiled(language)?;
    Some(Highlights { spans: spans(compiled, text), scopes: scopes(language, compiled, text) })
}

fn spans(compiled: &Compiled, text: &str) -> Vec<Span> {
    let mut highlighter = Highlighter::new();
    let Ok(events) = highlighter.highlight(&compiled.highlight, text.as_bytes(), None, None, |name| compiled_injection(name).map(|c| c as &HighlightConfiguration)) else { return vec![] };
    let mut out = Vec::new();
    let mut stack: Vec<u32> = Vec::new();
    let mut utf16 = Utf16::new(text);
    for event in events {
        match event {
            Ok(HighlightEvent::HighlightStart(h)) => stack.push(h.0 as u32),
            Ok(HighlightEvent::HighlightEnd) => {
                stack.pop();
            }
            Ok(HighlightEvent::Source { start, end }) => {
                if let Some(&capture) = stack.last() {
                    let (start, end) = (utf16.at(start), utf16.at(end));
                    match out.last_mut() {
                        Some(Span { end: last, capture: same, .. }) if *last == start && *same == capture => *last = end,
                        _ => out.push(Span { start, end, capture }),
                    }
                }
            }
            Err(_) => break,
        }
    }
    out
}

fn compiled_injection(name: &str) -> Option<&'static HighlightConfiguration> {
    let language = languages::injected(name)?;
    compiled(language).map(|c| &c.highlight)
}

fn scopes(language: &str, compiled: &Compiled, text: &str) -> Vec<Scope> {
    let Some(query) = &compiled.scopes else { return vec![] };
    let grammar = LANGUAGES.iter().find(|l| l.name == language).map(|l| (l.grammar)());
    let mut parser = Parser::new();
    if grammar.is_none_or(|g| parser.set_language(&g).is_err()) {
        return vec![];
    }
    let Some(tree) = parser.parse(text, None) else { return vec![] };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), text.as_bytes());
    let mut out: Vec<Scope> = Vec::new();
    while let Some(found) = matches.next() {
        for capture in found.captures() {
            let (first, last) = (capture.node.start_position().row as u32 + 1, capture.node.end_position().row as u32 + 1);
            if last > first {
                out.push(Scope { first, last });
            }
        }
    }
    out.sort_by(|a, b| a.first.cmp(&b.first).then(b.last.cmp(&a.last)));
    out.dedup_by(|later, earlier| later.first == earlier.first);
    out
}

struct Utf16<'a> {
    text: &'a str,
    byte: usize,
    unit: u32,
}

impl<'a> Utf16<'a> {
    fn new(text: &'a str) -> Self {
        Utf16 { text, byte: 0, unit: 0 }
    }

    fn at(&mut self, byte: usize) -> u32 {
        if byte < self.byte {
            self.byte = 0;
            self.unit = 0;
        }
        let end = byte.min(self.text.len());
        let mut cut = end;
        while !self.text.is_char_boundary(cut) {
            cut -= 1;
        }
        self.unit += self.text[self.byte..cut].encode_utf16().count() as u32;
        self.byte = cut;
        self.unit
    }
}
