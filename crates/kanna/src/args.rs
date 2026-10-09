use kmux_client::{Args, Failure};
use std::collections::VecDeque;

pub struct Argv {
    options: Args,
    rest: VecDeque<String>,
}

impl Argv {
    pub fn new(mut words: Vec<String>) -> Argv {
        let rest = match words.iter().position(|word| word == "--") {
            Some(i) => words.split_off(i).into_iter().skip(1).collect(),
            None => VecDeque::new(),
        };
        Argv { options: Args::new(words), rest }
    }

    pub fn flag(&mut self, name: &str) -> bool {
        self.options.flag(name)
    }

    pub fn option(&mut self, name: &str) -> Result<Option<String>, Failure> {
        self.options.option(name)
    }

    pub fn positional(&mut self) -> Option<String> {
        self.options.positional().or_else(|| self.rest.pop_front())
    }

    pub fn rest(&mut self) -> Option<String> {
        let mut words: Vec<String> = self.options.rest().into_iter().collect();
        words.extend(self.rest.drain(..));
        (!words.is_empty()).then(|| words.join(" "))
    }

    pub fn first(&self) -> Option<&str> {
        self.options.first().or(self.rest.front().map(String::as_str))
    }
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
    fn the_rest_joins_both_sides_of_the_double_dash() {
        let mut argv = args(&["p1", "ls", "--color", "--", "--json", "x"]);
        assert_eq!(argv.positional().as_deref(), Some("p1"));
        assert_eq!(argv.rest().as_deref(), Some("ls --color --json x"));
        assert_eq!(argv.rest(), None);
    }
}
