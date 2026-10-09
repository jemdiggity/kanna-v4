use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KmuxError {
    pub code: String,
    pub message: String,
}

impl KmuxError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> KmuxError {
        KmuxError { code: code.into(), message: message.into() }
    }
}

impl fmt::Display for KmuxError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} [{}]", self.message, self.code)
    }
}

impl std::error::Error for KmuxError {}
