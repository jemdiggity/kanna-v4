use std::path::{Path, PathBuf};

const ZSHENV: &str = include_str!("zshenv.zsh");

pub fn env(state_dir: &Path, zdotdir: Option<String>) -> Vec<(String, String)> {
    let Ok(dir) = install(state_dir) else { return vec![] };
    let mut env = vec![("ZDOTDIR".to_string(), dir.to_string_lossy().into_owned())];
    env.extend(zdotdir.map(|old| ("KANNA_ZSH_ZDOTDIR".to_string(), old)));
    env
}

fn install(state_dir: &Path) -> std::io::Result<PathBuf> {
    let dir = state_dir.join("shell/zsh");
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(".zshenv");
    if std::fs::read_to_string(&file).ok().as_deref() != Some(ZSHENV) {
        std::fs::write(&file, ZSHENV)?;
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminals_start_zsh_in_kannas_folder_and_keep_the_users() {
        let dir = std::env::temp_dir().join(format!("kanna-shell-{}", std::process::id()));
        let env = env(&dir, None);
        assert_eq!(env, [("ZDOTDIR".to_string(), dir.join("shell/zsh").to_string_lossy().into_owned())]);
        assert_eq!(std::fs::read_to_string(dir.join("shell/zsh/.zshenv")).unwrap(), ZSHENV);
        let env = super::env(&dir, Some("/u/zsh".into()));
        assert_eq!(env[1], ("KANNA_ZSH_ZDOTDIR".to_string(), "/u/zsh".to_string()));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
