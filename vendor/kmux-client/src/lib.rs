//! Shared by the `kmux` and `kanna` command lines: the control-socket client,
//! exit codes and a small argument reader.

mod args;
mod client;

pub use args::Args;
pub use client::{instance_socket, socket_dir, socket_path, valid_instance, Error, Kmux, Target, DEFAULT_INSTANCE};

/// Exit codes, shared by the kmux and kanna CLIs.
pub mod exit {
    pub const FAILED: u8 = 1;
    pub const USAGE: u8 = 2;
    pub const UNREACHABLE: u8 = 3;
    pub const NOT_FOUND: u8 = 4;
    pub const UNSUPPORTED: u8 = 5;
}

/// An error to print before exiting with `code`.
#[derive(Debug)]
pub struct Failure {
    pub code: u8,
    pub message: String,
}

pub fn fail(code: u8, message: impl Into<String>) -> Failure {
    Failure { code, message: message.into() }
}

impl From<Error> for Failure {
    fn from(error: Error) -> Failure {
        match error {
            Error::Unreachable(message) => fail(exit::UNREACHABLE, message),
            Error::Mux { code, message } => {
                let exit = match code.as_str() {
                    "not_found" => exit::NOT_FOUND,
                    "bad_request" if message.starts_with("unknown command") => exit::UNSUPPORTED,
                    "bad_request" | "layout_invalid" => exit::USAGE,
                    _ => exit::FAILED,
                };
                fail(exit, format!("{message} [{code}]"))
            }
        }
    }
}
