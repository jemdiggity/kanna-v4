use std::ffi::OsStr;
use std::path::{Path, PathBuf};

const ZSHENV: &str = include_str!("zshenv.zsh");
const BASH: &str = include_str!("kanna.bash");

pub fn env(state_dir: &Path, shell: &OsStr, zdotdir: Option<String>) -> Vec<(String, String)> {
    let name = Path::new(shell).file_name().and_then(OsStr::to_str).unwrap_or_default().trim_start_matches('-');
    let text = |path: PathBuf| path.to_string_lossy().into_owned();
    match name {
        "zsh" => {
            let Some(dir) = install(state_dir, "shell/zsh/.zshenv", ZSHENV).ok().and_then(|file| file.parent().map(Path::to_path_buf)) else { return vec![] };
            let mut env = vec![("ZDOTDIR".to_string(), text(dir))];
            env.extend(zdotdir.map(|old| ("KANNA_ZSH_ZDOTDIR".to_string(), old)));
            env
        }
        "bash" => match install(state_dir, "shell/bash/kanna.bash", BASH) {
            Ok(file) => vec![("KANNA_BASH_INIT".to_string(), text(file)), ("PROMPT_COMMAND".to_string(), ". \"$KANNA_BASH_INIT\"".to_string())],
            Err(_) => vec![],
        },
        _ => vec![],
    }
}

fn install(state_dir: &Path, name: &str, content: &str) -> std::io::Result<PathBuf> {
    let file = state_dir.join(name);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if std::fs::read_to_string(&file).ok().as_deref() != Some(content) {
        std::fs::write(&file, content)?;
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("kanna-shell-{name}-{}", std::process::id()))
    }

    #[test]
    fn zsh_starts_in_kannas_folder_and_keeps_the_users() {
        let dir = folder("zsh");
        let env = env(&dir, OsStr::new("/bin/zsh"), None);
        assert_eq!(env, [("ZDOTDIR".to_string(), dir.join("shell/zsh").to_string_lossy().into_owned())]);
        assert_eq!(std::fs::read_to_string(dir.join("shell/zsh/.zshenv")).unwrap(), ZSHENV);
        let env = super::env(&dir, OsStr::new("-zsh"), Some("/u/zsh".into()));
        assert_eq!(env[1], ("KANNA_ZSH_ZDOTDIR".to_string(), "/u/zsh".to_string()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bash_runs_kannas_file_at_its_first_prompt() {
        let dir = folder("bash");
        let env = env(&dir, OsStr::new("/opt/homebrew/bin/bash"), Some("/u/zsh".into()));
        let file = dir.join("shell/bash/kanna.bash").to_string_lossy().into_owned();
        assert_eq!(env, [("KANNA_BASH_INIT".to_string(), file.clone()), ("PROMPT_COMMAND".to_string(), ". \"$KANNA_BASH_INIT\"".to_string())]);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), BASH);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn other_shells_are_left_alone() {
        assert!(env(&folder("fish"), OsStr::new("/opt/homebrew/bin/fish"), None).is_empty());
    }
}
