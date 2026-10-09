use crate::{exit, fail, Failure};

/// A small argument reader: flags and options may appear anywhere, as
/// `--name value` or `--name=value`.
pub struct Args(Vec<String>);

impl Args {
    pub fn new(args: Vec<String>) -> Args {
        Args(args)
    }

    pub fn flag(&mut self, name: &str) -> bool {
        let before = self.0.len();
        self.0.retain(|a| a != name);
        self.0.len() != before
    }

    pub fn option(&mut self, name: &str) -> Result<Option<String>, Failure> {
        let prefix = format!("{name}=");
        if let Some(i) = self.0.iter().position(|a| a.starts_with(&prefix)) {
            return Ok(Some(self.0.remove(i)[prefix.len()..].to_string()));
        }
        let Some(i) = self.0.iter().position(|a| a == name) else { return Ok(None) };
        if i + 1 >= self.0.len() {
            return Err(fail(exit::USAGE, format!("{name} needs a value")));
        }
        self.0.remove(i);
        Ok(Some(self.0.remove(i)))
    }

    /// The next argument that isn't an option. `--` ends options.
    pub fn positional(&mut self) -> Option<String> {
        if let Some(i) = self.0.iter().position(|a| a == "--") {
            if i == 0 || self.0[..i].iter().all(|a| a.starts_with("--")) {
                self.0.remove(i);
                return (i < self.0.len()).then(|| self.0.remove(i));
            }
        }
        let i = self.0.iter().position(|a| !a.starts_with("--") || a == "-")?;
        Some(self.0.remove(i))
    }

    /// The rest of the arguments, joined by spaces (for free text).
    pub fn rest(&mut self) -> Option<String> {
        let words: Vec<String> = std::mem::take(&mut self.0);
        (!words.is_empty()).then(|| words.join(" "))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn first(&self) -> Option<&str> {
        self.0.first().map(String::as_str)
    }
}
