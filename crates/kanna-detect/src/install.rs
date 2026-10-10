use crate::version::numbers;
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const MAX_READ: u64 = 64 * 1024;
const LEVELS: usize = 6;

const PACKAGES: &[(&str, &[&str])] = &[
    ("claude", &["@anthropic-ai/claude-code"]),
    ("codex", &["@openai/codex"]),
    ("opencode", &["opencode-ai"]),
    ("copilot", &["@github/copilot"]),
];

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    path: PathBuf,
    inode: u64,
    modified: i64,
    size: u64,
}

fn cache() -> &'static Mutex<HashMap<(String, Key), Option<String>>> {
    static CACHE: OnceLock<Mutex<HashMap<(String, Key), Option<String>>>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

pub fn version(agent: &str, executable: &Path) -> Option<String> {
    if !executable.is_absolute() {
        return None;
    }
    let meta = fs::metadata(executable).ok().filter(|m| m.is_file())?;
    let key = (agent.to_string(), Key { path: executable.to_path_buf(), inode: meta.ino(), modified: meta.mtime(), size: meta.len() });
    if let Some(found) = cache().lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return found.clone();
    }
    let found = read(agent, executable);
    cache().lock().unwrap_or_else(|e| e.into_inner()).insert(key, found.clone());
    found
}

fn read(agent: &str, executable: &Path) -> Option<String> {
    match agent {
        "claude" => versioned_file(executable, "versions").or_else(|| app_bundle(executable)).or_else(|| package(agent, executable)),
        "codex" => package(agent, executable).or_else(|| release_folder(executable)),
        _ => package(agent, executable),
    }
}

fn clean(text: &str) -> Option<String> {
    let parts = numbers(text)?;
    (parts.len() >= 2 && parts.len() <= 4).then(|| parts.iter().map(u64::to_string).collect::<Vec<_>>().join("."))
}

fn versioned_file(executable: &Path, folder: &str) -> Option<String> {
    let parent = executable.parent()?;
    if parent.file_name()?.to_str()? != folder {
        return None;
    }
    let name = executable.file_name()?.to_str()?;
    name.chars().all(|c| c.is_ascii_digit() || c == '.').then(|| clean(name)).flatten()
}

fn release_folder(executable: &Path) -> Option<String> {
    let bin = executable.parent()?;
    let release = bin.parent()?;
    if bin.file_name()?.to_str()? != "bin" || release.parent()?.file_name()?.to_str()? != "releases" {
        return None;
    }
    let name = release.file_name()?.to_str()?;
    let head = name.split('-').next()?;
    head.chars().all(|c| c.is_ascii_digit() || c == '.').then(|| clean(head)).flatten()
}

fn app_bundle(executable: &Path) -> Option<String> {
    let contents = executable.parent()?.parent()?;
    if contents.file_name()?.to_str()? != "Contents" {
        return None;
    }
    let plist = small_file(&contents.join("Info.plist"))?;
    let at = plist.find("<key>CFBundleShortVersionString</key>")?;
    let rest = &plist[at..];
    let start = rest.find("<string>")? + "<string>".len();
    let end = rest[start..].find("</string>")? + start;
    clean(&rest[start..end])
}

fn package(agent: &str, executable: &Path) -> Option<String> {
    let names = PACKAGES.iter().find(|(a, _)| *a == agent)?.1;
    let mut dir = executable.parent();
    for _ in 0..LEVELS {
        let here = dir?;
        for file in ["package.json", "codex-package.json"] {
            if let Some(text) = small_file(&here.join(file)) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                    let name = json.get("name").and_then(|n| n.as_str()).unwrap_or_default();
                    if names.contains(&name) {
                        if let Some(version) = json.get("version").and_then(|v| v.as_str()).and_then(clean) {
                            return Some(version);
                        }
                    }
                }
            }
        }
        dir = here.parent();
    }
    None
}

fn small_file(path: &Path) -> Option<String> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_file() || meta.len() > MAX_READ {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    (bytes.len() as u64 <= MAX_READ).then(|| String::from_utf8(bytes).ok()).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kanna-install-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn file(path: &Path, text: &str) -> PathBuf {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        path.to_path_buf()
    }

    #[test]
    fn claude_s_native_install_names_its_version() {
        let dir = scratch("claude");
        let marker = dir.join("ran");
        let exe = file(&dir.join(".local/share/claude/versions/2.1.296"), &format!("#!/bin/sh\ntouch {}\n", marker.display()));
        assert_eq!(version("claude", &exe), Some("2.1.296".into()));
        assert!(!marker.exists(), "the agent was run");
        let app = file(&dir.join("ClaudeCode.app/Contents/MacOS/claude"), "");
        file(&dir.join("ClaudeCode.app/Contents/Info.plist"), "<plist><dict><key>CFBundleShortVersionString</key><string>2.1.300</string></dict></plist>");
        assert_eq!(version("claude", &app), Some("2.1.300".into()));
        let other = file(&dir.join("bin/claude"), "");
        assert_eq!(version("claude", &other), None);
    }

    #[test]
    fn codex_s_npm_and_release_installs_name_their_version() {
        let dir = scratch("codex");
        let root = dir.join("lib/node_modules/@openai/codex");
        file(&root.join("package.json"), r#"{"name":"@openai/codex","version":"0.162.0"}"#);
        let platform = root.join("node_modules/@openai/codex-darwin-arm64");
        file(&platform.join("package.json"), r#"{"name":"@openai/codex","version":"0.162.0-darwin-arm64"}"#);
        let exe = file(&platform.join("vendor/aarch64-apple-darwin/bin/codex"), "");
        assert_eq!(version("codex", &exe), Some("0.162.0".into()));
        let release = file(&dir.join(".codex/packages/app-server-daemon/releases/0.162.1-aarch64-apple-darwin/bin/codex"), "");
        assert_eq!(version("codex", &release), Some("0.162.1".into()));
        let stranger = file(&dir.join("x/bin/codex"), "");
        file(&dir.join("x/package.json"), r#"{"name":"not-codex","version":"9.9.9"}"#);
        assert_eq!(version("codex", &stranger), None);
    }

    #[test]
    fn big_or_linked_files_are_not_read() {
        let dir = scratch("limits");
        let exe = file(&dir.join("pkg/bin/codex"), "");
        file(&dir.join("pkg/real.json"), r#"{"name":"@openai/codex","version":"1.2.3"}"#);
        std::os::unix::fs::symlink(dir.join("pkg/real.json"), dir.join("pkg/package.json")).unwrap();
        assert_eq!(version("codex", &exe), None);
        let exe = file(&dir.join("big/bin/codex"), "");
        file(&dir.join("big/package.json"), &format!(r#"{{"name":"@openai/codex","version":"1.2.3","pad":"{}"}}"#, "x".repeat(70 * 1024)));
        assert_eq!(version("codex", &exe), None);
        assert_eq!(version("codex", Path::new("bin/codex")), None);
    }

    #[test]
    fn the_status_path_runs_nothing() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut sources: Vec<PathBuf> = fs::read_dir(root.join("src")).unwrap().flatten().map(|e| e.path()).collect();
        sources.extend(["status.rs", "status_command.rs"].map(|f| root.join("../kanna-daemon/src").join(f)));
        for source in sources {
            let text = fs::read_to_string(&source).unwrap();
            for forbidden in ["process::Command", "Command::new", "posix_spawn", "execv", "libc::fork"] {
                let found = text.lines().any(|l| l.contains(forbidden) && !l.contains("forbidden"));
                assert!(!found, "{} uses {forbidden}", source.display());
            }
        }
    }
}
