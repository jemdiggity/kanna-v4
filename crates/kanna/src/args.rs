use kanna_client::{Args, Failure};
use std::collections::VecDeque;

pub struct Argv {
    options: Args,
    rest: VecDeque<String>,
    doc: Option<String>,
}

impl Argv {
    pub fn new(mut words: Vec<String>) -> Argv {
        let rest = match words.iter().position(|word| word == "--") {
            Some(i) => words.split_off(i).into_iter().skip(1).collect(),
            None => VecDeque::new(),
        };
        let doc = take_doc(&mut words);
        Argv { options: Args::new(words), rest, doc }
    }

    pub fn doc(&mut self) -> Option<String> {
        self.doc.take()
    }

    pub fn flag(&mut self, name: &str) -> bool {
        self.options.flag(name)
    }

    pub fn option(&mut self, name: &str) -> Result<Option<String>, Failure> {
        self.options.option(name)
    }

    pub fn optional(&mut self, name: &str, takes: impl Fn(&str) -> bool) -> Option<Option<String>> {
        self.options.optional(name, takes)
    }

    pub fn positional(&mut self) -> Option<String> {
        self.options.positional().or_else(|| self.rest.pop_front())
    }

    pub fn rest(&mut self) -> Option<String> {
        let mut words: Vec<String> = self.options.rest().into_iter().collect();
        words.extend(self.rest.drain(..));
        (!words.is_empty()).then(|| words.join(" "))
    }

    pub fn remaining(&mut self) -> Vec<String> {
        let mut words = self.options.take();
        if !self.rest.is_empty() {
            words.push("--".into());
            words.extend(self.rest.drain(..));
        }
        words
    }

    pub fn positionals(&mut self) -> Vec<String> {
        std::iter::from_fn(|| self.positional()).collect()
    }

    pub fn first(&self) -> Option<&str> {
        self.options.first().or(self.rest.front().map(String::as_str)).or(self.doc.as_ref().map(|_| "--json DOC"))
    }
}

fn take_doc(words: &mut Vec<String>) -> Option<String> {
    let object = |word: &str| word.trim_start().starts_with('{');
    if let Some(i) = words.iter().position(|w| w.strip_prefix("--json=").is_some_and(object)) {
        return Some(words.remove(i)["--json=".len()..].to_string());
    }
    let i = words.windows(2).position(|pair| pair[0] == "--json" && object(&pair[1]))?;
    words.remove(i);
    Some(words.remove(i))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Argv {
        Argv::new(list.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn a_double_dash_ends_options() {
        let mut argv = args(&["--json", "send", "p1", "--", "--json", "--mux"]);
        assert!(argv.flag("--json"));
        assert!(!argv.flag("--json"));
        assert_eq!(argv.option("--mux").unwrap(), None);
        assert_eq!(argv.positional().as_deref(), Some("send"));
        assert_eq!(argv.positional().as_deref(), Some("p1"));
        assert_eq!(argv.first(), Some("--json"));
        assert_eq!(argv.positional().as_deref(), Some("--json"));
        assert_eq!(argv.positional().as_deref(), Some("--mux"));
        assert_eq!(argv.positional(), None);
    }

    #[test]
    fn a_json_object_after_json_is_the_command_s_doc_and_a_bare_json_is_still_the_output_flag() {
        let mut argv = args(&["quiz", "answer", "--json", "{\"pick\": 2}", "--json", "--right"]);
        assert_eq!(argv.doc().as_deref(), Some("{\"pick\": 2}"));
        assert!(argv.flag("--json"));
        assert!(argv.flag("--right"));
        assert_eq!(args(&["--json={}", "list"]).doc().as_deref(), Some("{}"));
        let mut unclaimed = args(&["list", "--json", "{}"]);
        unclaimed.positional();
        assert_eq!(unclaimed.first(), Some("--json DOC"));
        assert_eq!(args(&["list", "--json"]).doc(), None);
    }

    #[test]
    fn what_remains_keeps_its_options_and_double_dash_in_order() {
        let mut argv = args(&["diff", "--merge-base", "main", "-U5", "--", "src", "--tab"]);
        assert_eq!(argv.positional().as_deref(), Some("diff"));
        assert_eq!(argv.remaining(), ["--merge-base", "main", "-U5", "--", "src", "--tab"]);
        assert!(argv.remaining().is_empty());
    }

    #[test]
    fn the_rest_joins_both_sides_of_the_double_dash() {
        let mut argv = args(&["p1", "ls", "--color", "--", "--json", "x"]);
        assert_eq!(argv.positional().as_deref(), Some("p1"));
        assert_eq!(argv.rest().as_deref(), Some("ls --color --json x"));
        assert_eq!(argv.rest(), None);
    }
}
