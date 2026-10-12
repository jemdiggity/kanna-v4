pub struct Language {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub grammar: fn() -> tree_sitter::Language,
    pub highlights: &'static str,
    pub injections: &'static str,
    pub locals: &'static str,
    pub scopes: &'static str,
}

pub const LANGUAGES: &[Language] = &[Language {
    name: "rust",
    extensions: &["rs"],
    grammar: || tree_sitter_rust::LANGUAGE.into(),
    highlights: tree_sitter_rust::HIGHLIGHTS_QUERY,
    injections: tree_sitter_rust::INJECTIONS_QUERY,
    locals: "",
    scopes: include_str!("../queries/rust-scopes.scm"),
}];

const INTERPRETERS: &[(&str, &str)] = &[];

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
