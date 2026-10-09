use crate::error::KmuxError;
use crate::instance::Instance;
use crate::json::{encode_pretty, Value};
use std::fs::{File, Metadata, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateFile {
    pub path: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Contents {
    None,
    State(Value),
    Refused(String),
}

const MAX_SIZE: u64 = 16 << 20;

pub(crate) fn os_message(error: &std::io::Error) -> String {
    match error.raw_os_error() {
        Some(code) => unsafe { std::ffi::CStr::from_ptr(libc::strerror(code)) }.to_string_lossy().into_owned(),
        None => error.to_string(),
    }
}

fn folder_of(path: &str) -> String {
    Path::new(path).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
}

fn uid() -> u32 {
    unsafe { libc::getuid() }
}

impl StateFile {
    pub fn new(path: &str) -> StateFile {
        StateFile { path: path.into() }
    }

    pub fn path_for(instance: &Instance) -> String {
        if instance.socket_path == Instance::socket_path_for(&instance.name) {
            return Path::new(&Instance::directory()).join(format!("state-{}.json", instance.name)).to_string_lossy().into_owned();
        }
        let socket = &instance.socket_path;
        format!("{}.state.json", socket.strip_suffix(".sock").unwrap_or(socket))
    }

    pub fn load(&self) -> Contents {
        let path = &self.path;
        let info = match std::fs::symlink_metadata(path) {
            Ok(info) => info,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Contents::None,
            Err(e) => return Contents::Refused(format!("can't read {path}: {}", os_message(&e))),
        };
        if let Some(problem) = StateFile::unsafe_file(&info, true) {
            return Contents::Refused(format!("{path} {problem}"));
        }
        if let Some(problem) = StateFile::unsafe_folder(&folder_of(path)) {
            return Contents::Refused(problem);
        }
        if info.len() > MAX_SIZE {
            return Contents::Refused(format!("{path} is too large"));
        }
        let mut data = vec![];
        let read = OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(path).and_then(|mut file| file.read_to_end(&mut data));
        if let Err(e) = read {
            return Contents::Refused(format!("can't read {path}: {}", os_message(&e)));
        }
        match serde_json::from_slice(&data) {
            Ok(state) => Contents::State(state),
            Err(_) => Contents::Refused(format!("{path} is not valid JSON")),
        }
    }

    fn unsafe_file(info: &Metadata, regular_file: bool) -> Option<String> {
        if regular_file && !info.file_type().is_file() {
            return Some("is not a regular file".into());
        }
        if info.uid() != uid() {
            return Some("belongs to another user".into());
        }
        if info.mode() & 0o077 != 0 {
            return Some(format!("can be read or written by other users (mode {:o})", info.mode() & 0o777));
        }
        None
    }

    fn unsafe_folder(folder: &str) -> Option<String> {
        let Ok(info) = std::fs::metadata(folder) else { return Some(format!("can't check {folder}")) };
        let sticky = info.mode() & 0o1000 != 0;
        if info.uid() != uid() && !(info.uid() == 0 && sticky) {
            return Some(format!("{folder} belongs to another user"));
        }
        if info.mode() & 0o022 != 0 && !sticky {
            return Some(format!("{folder} can be written by other users"));
        }
        None
    }

    pub fn write(&self, state: &Value) -> Result<(), KmuxError> {
        let failure = |message: String, e: &std::io::Error| KmuxError::new("io_error", format!("{message}: {}", os_message(e)));
        let folder = folder_of(&self.path);
        if !folder.is_empty() {
            std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&folder).map_err(|e| failure(format!("can't create {folder}"), &e))?;
        }
        let data = encode_pretty(state);
        let temporary = format!("{}.tmp-{}", self.path, std::process::id());
        let _ = std::fs::remove_file(&temporary);
        let mut file: File = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)
            .map_err(|e| failure(format!("can't write {temporary}"), &e))?;
        let saved = file.write_all(data.as_bytes()).and_then(|_| file.sync_all()).and_then(|_| {
            drop(file);
            std::fs::rename(&temporary, &self.path)
        });
        if let Err(e) = saved {
            let _ = std::fs::remove_file(&temporary);
            return Err(failure(format!("can't save {}", self.path), &e));
        }
        Ok(())
    }

    pub fn set_aside(&self) -> Option<String> {
        let aside = format!("{}.bad", self.path);
        std::fs::rename(&self.path, &aside).ok().map(|_| aside)
    }
}
