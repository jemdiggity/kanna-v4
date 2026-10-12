pub struct Language {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub grammar: fn() -> tree_sitter::Language,
    pub highlights: &'static [&'static str],
    pub injections: &'static [&'static str],
    pub locals: &'static [&'static str],
    pub scopes: &'static str,
    pub neovim: bool,
}

unsafe extern "C" {
    fn tree_sitter_vue() -> *const ();
}

const VUE: tree_sitter_language::LanguageFn = unsafe { tree_sitter_language::LanguageFn::from_raw(tree_sitter_vue) };

const NONE: &[&str] = &[];

const HTML_TAGS_HIGHLIGHTS: &str = include_str!("../vendor/tree-sitter-vue/queries/html_tags/highlights.scm");
const HTML_TAGS_INJECTIONS: &str = include_str!("../vendor/tree-sitter-vue/queries/html_tags/injections.scm");

pub const LANGUAGES: &[Language] = &[
    Language {
        name: "rust",
        extensions: &["rs"],
        grammar: || tree_sitter_rust::LANGUAGE.into(),
        highlights: &[tree_sitter_rust::HIGHLIGHTS_QUERY],
        injections: &[tree_sitter_rust::INJECTIONS_QUERY],
        locals: NONE,
        scopes: include_str!("../queries/rust-scopes.scm"),
        neovim: false,
    },
    Language {
        name: "swift",
        extensions: &["swift"],
        grammar: || tree_sitter_swift::LANGUAGE.into(),
        highlights: &[tree_sitter_swift::HIGHLIGHTS_QUERY],
        injections: &[tree_sitter_swift::INJECTIONS_QUERY],
        locals: &[tree_sitter_swift::LOCALS_QUERY],
        scopes: include_str!("../queries/swift-scopes.scm"),
        neovim: false,
    },
    Language {
        name: "typescript",
        extensions: &["ts", "mts", "cts"],
        grammar: || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        highlights: &[tree_sitter_javascript::HIGHLIGHT_QUERY, tree_sitter_typescript::HIGHLIGHTS_QUERY],
        injections: &[tree_sitter_javascript::INJECTIONS_QUERY],
        locals: &[tree_sitter_javascript::LOCALS_QUERY, tree_sitter_typescript::LOCALS_QUERY],
        scopes: include_str!("../queries/typescript-scopes.scm"),
        neovim: false,
    },
    Language {
        name: "tsx",
        extensions: &["tsx"],
        grammar: || tree_sitter_typescript::LANGUAGE_TSX.into(),
        highlights: &[tree_sitter_javascript::HIGHLIGHT_QUERY, tree_sitter_javascript::JSX_HIGHLIGHT_QUERY, tree_sitter_typescript::HIGHLIGHTS_QUERY],
        injections: &[tree_sitter_javascript::INJECTIONS_QUERY],
        locals: &[tree_sitter_javascript::LOCALS_QUERY, tree_sitter_typescript::LOCALS_QUERY],
        scopes: include_str!("../queries/tsx-scopes.scm"),
        neovim: false,
    },
    Language {
        name: "javascript",
        extensions: &["js", "mjs", "cjs", "jsx"],
        grammar: || tree_sitter_javascript::LANGUAGE.into(),
        highlights: &[tree_sitter_javascript::HIGHLIGHT_QUERY, tree_sitter_javascript::JSX_HIGHLIGHT_QUERY],
        injections: &[tree_sitter_javascript::INJECTIONS_QUERY],
        locals: &[tree_sitter_javascript::LOCALS_QUERY],
        scopes: include_str!("../queries/javascript-scopes.scm"),
        neovim: false,
    },
    Language {
        name: "python",
        extensions: &["py", "pyi", "pyw"],
        grammar: || tree_sitter_python::LANGUAGE.into(),
        highlights: &[tree_sitter_python::HIGHLIGHTS_QUERY],
        injections: NONE,
        locals: NONE,
        scopes: include_str!("../queries/python-scopes.scm"),
        neovim: false,
    },
    Language {
        name: "go",
        extensions: &["go"],
        grammar: || tree_sitter_go::LANGUAGE.into(),
        highlights: &[tree_sitter_go::HIGHLIGHTS_QUERY],
        injections: NONE,
        locals: NONE,
        scopes: include_str!("../queries/go-scopes.scm"),
        neovim: false,
    },
    Language {
        name: "c",
        extensions: &["c", "h"],
        grammar: || tree_sitter_c::LANGUAGE.into(),
        highlights: &[tree_sitter_c::HIGHLIGHT_QUERY],
        injections: NONE,
        locals: NONE,
        scopes: include_str!("../queries/c-scopes.scm"),
        neovim: false,
    },
    Language {
        name: "cpp",
        extensions: &["cc", "cpp", "cxx", "c++", "hpp", "hh", "hxx", "h++", "ipp", "inl"],
        grammar: || tree_sitter_cpp::LANGUAGE.into(),
        highlights: &[tree_sitter_cpp::HIGHLIGHT_QUERY, tree_sitter_c::HIGHLIGHT_QUERY],
        injections: NONE,
        locals: NONE,
        scopes: include_str!("../queries/cpp-scopes.scm"),
        neovim: false,
    },
    Language {
        name: "bash",
        extensions: &["sh", "bash", "zsh", "ksh", "bashrc", "zshrc", "profile", "bash_profile", "zprofile", "zshenv"],
        grammar: || tree_sitter_bash::LANGUAGE.into(),
        highlights: &[tree_sitter_bash::HIGHLIGHT_QUERY],
        injections: NONE,
        locals: NONE,
        scopes: include_str!("../queries/bash-scopes.scm"),
        neovim: false,
    },
    Language {
        name: "json",
        extensions: &["json"],
        grammar: || tree_sitter_json::LANGUAGE.into(),
        highlights: &[tree_sitter_json::HIGHLIGHTS_QUERY],
        injections: NONE,
        locals: NONE,
        scopes: include_str!("../queries/json-scopes.scm"),
        neovim: false,
    },
    Language {
        name: "toml",
        extensions: &["toml"],
        grammar: || tree_sitter_toml_ng::LANGUAGE.into(),
        highlights: &[tree_sitter_toml_ng::HIGHLIGHTS_QUERY],
        injections: NONE,
        locals: NONE,
        scopes: include_str!("../queries/toml-scopes.scm"),
        neovim: false,
    },
    Language {
        name: "yaml",
        extensions: &["yaml", "yml"],
        grammar: || tree_sitter_yaml::LANGUAGE.into(),
        highlights: &[tree_sitter_yaml::HIGHLIGHTS_QUERY],
        injections: NONE,
        locals: NONE,
        scopes: include_str!("../queries/yaml-scopes.scm"),
        neovim: false,
    },
    Language {
        name: "html",
        extensions: &["html", "htm", "xhtml"],
        grammar: || tree_sitter_html::LANGUAGE.into(),
        highlights: &[tree_sitter_html::HIGHLIGHTS_QUERY],
        injections: &[tree_sitter_html::INJECTIONS_QUERY],
        locals: NONE,
        scopes: include_str!("../queries/html-scopes.scm"),
        neovim: false,
    },
    Language {
        name: "css",
        extensions: &["css"],
        grammar: || tree_sitter_css::LANGUAGE.into(),
        highlights: &[tree_sitter_css::HIGHLIGHTS_QUERY],
        injections: NONE,
        locals: NONE,
        scopes: include_str!("../queries/css-scopes.scm"),
        neovim: false,
    },
    Language {
        name: "svelte",
        extensions: &["svelte"],
        grammar: || tree_sitter_svelte_ng::LANGUAGE.into(),
        highlights: &[HTML_TAGS_HIGHLIGHTS, tree_sitter_svelte_ng::HIGHLIGHTS_QUERY],
        injections: &[HTML_TAGS_INJECTIONS, tree_sitter_svelte_ng::INJECTIONS_QUERY],
        locals: &[tree_sitter_svelte_ng::LOCALS_QUERY],
        scopes: include_str!("../queries/svelte-scopes.scm"),
        neovim: true,
    },
    Language {
        name: "vue",
        extensions: &["vue"],
        grammar: || VUE.into(),
        highlights: &[HTML_TAGS_HIGHLIGHTS, include_str!("../vendor/tree-sitter-vue/queries/vue/highlights.scm")],
        injections: &[HTML_TAGS_INJECTIONS, include_str!("../vendor/tree-sitter-vue/queries/vue/injections.scm")],
        locals: NONE,
        scopes: include_str!("../queries/vue-scopes.scm"),
        neovim: true,
    },
];

const INTERPRETERS: &[(&str, &str)] = &[("bash", "bash"), ("zsh", "bash"), ("sh", "bash"), ("dash", "bash"), ("ksh", "bash"), ("python", "python"), ("node", "javascript"), ("deno", "typescript"), ("bun", "typescript")];

pub fn language_for(file_name: &str, first_line: &str) -> Option<&'static str> {
    let lower = file_name.to_lowercase();
    let extension = lower.rsplit_once('.').map(|(_, ext)| ext);
    if let Some(found) = extension.and_then(|ext| LANGUAGES.iter().find(|l| l.extensions.contains(&ext))) {
        return Some(found.name);
    }
    let shebang = first_line.strip_prefix("#!")?;
    let mut words = shebang.split_whitespace();
    let program = words.next()?.rsplit('/').next()?;
    let program = if program == "env" { words.find(|w| !w.starts_with('-'))? } else { program };
    INTERPRETERS.iter().find(|(prefix, _)| program.starts_with(prefix)).map(|(_, name)| *name)
}

pub fn injected(name: &str) -> Option<&'static str> {
    let lower = name.to_lowercase();
    LANGUAGES.iter().find(|l| l.name == lower || l.extensions.contains(&lower.as_str())).map(|l| l.name)
}

pub fn neovim(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    for line in query.lines() {
        let line = line.replace("#not-lua-match?", "#not-match?").replace("#lua-match?", "#match?").replace(" @nospell", "").replace(" @spell", "");
        if line.contains("#match?") || line.contains("#not-match?") {
            out.push_str(&lua_pattern_to_regex(&line));
        } else {
            out.push_str(&line);
        }
        out.push('\n');
    }
    out
}

fn lua_pattern_to_regex(line: &str) -> String {
    let Some(start) = line.find('"') else { return line.into() };
    let Some(end) = line.rfind('"').filter(|end| *end > start) else { return line.into() };
    let pattern = &line[start + 1..end];
    let mut regex = String::new();
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        if matches!(c, '{' | '}' | '|') {
            regex.push_str("\\\\");
            regex.push(c);
            continue;
        }
        if c != '%' {
            regex.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => regex.push_str("\\\\s"),
            Some('d') => regex.push_str("\\\\d"),
            Some('w') => regex.push_str("\\\\w"),
            Some('a') => regex.push_str("[a-zA-Z]"),
            Some(other) => {
                regex.push_str("\\\\");
                regex.push(other);
            }
            None => {}
        }
    }
    format!("{}\"{}\"{}", &line[..start], regex, &line[end + 1..])
}
