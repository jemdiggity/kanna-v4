use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.parent().and_then(Path::parent).expect("crates/kanna-vt is two levels below the repository").to_path_buf();
    let target = env::var("TARGET").unwrap();
    let host = env::var("HOST").unwrap();
    let env_sh = root.join("scripts/env.sh");
    println!("cargo::rerun-if-env-changed=KANNA_GHOSTTY_VT_DIR");
    println!("cargo::rerun-if-changed={}", env_sh.display());
    println!("cargo::rerun-if-changed=csrc/abi.c");
    println!("cargo::rustc-check-cfg=cfg(kanna_vt_abi)");

    let pinned = pinned_commit(&env_sh);
    let (dir, explicit) = match env::var_os("KANNA_GHOSTTY_VT_DIR") {
        Some(dir) => (PathBuf::from(dir), true),
        None => (built_here_or_in_main_checkout(&root, &target, &pinned), false),
    };
    let lib = dir.join("lib/libghostty-vt.a");
    let rev_file = dir.join("ghostty.rev");
    println!("cargo::rerun-if-changed={}", lib.display());
    println!("cargo::rerun-if-changed={}", rev_file.display());

    if !lib.exists() {
        let fix = if explicit {
            format!("KANNA_GHOSTTY_VT_DIR is {}, which has no lib/libghostty-vt.a", dir.display())
        } else if target == host {
            "run scripts/build-ghostty-vt.sh".to_string()
        } else {
            format!("run scripts/build-ghostty-vt.sh {target}")
        };
        let message = format!("libghostty-vt for {target} isn't built (no {}): {fix}", lib.display());
        if target != host {
            println!("cargo::warning={message}");
            println!("cargo::rustc-env=KANNA_GHOSTTY_VT_COMMIT=unknown");
            return;
        }
        panic!("\n\n{message}\n\n");
    }

    let built = fs::read_to_string(&rev_file).map(|s| s.trim().to_string()).unwrap_or_default();
    if built != pinned {
        let message = format!(
            "{} was built from Ghostty {}, but scripts/env.sh pins {pinned}: \
             snapshots must come from the app's Ghostty commit",
            dir.display(),
            if built.is_empty() { "(unknown: no ghostty.rev)" } else { &built },
        );
        if explicit {
            println!("cargo::warning={message}");
        } else {
            panic!("\n\n{message}. Run scripts/build-ghostty-vt.sh{}.\n\n", if target == host { String::new() } else { format!(" {target}") });
        }
    }
    println!("cargo::rustc-env=KANNA_GHOSTTY_VT_COMMIT={built}");
    println!("cargo::rustc-link-search=native={}", dir.join("lib").display());
    println!("cargo::rustc-link-lib=static=ghostty-vt");

    if target == host {
        cc::Build::new()
            .file("csrc/abi.c")
            .include(dir.join("include"))
            .std("c11")
            .flag_if_supported("-Werror=incompatible-function-pointer-types")
            .flag_if_supported("-Werror=incompatible-pointer-types")
            .compile("kanna_vt_abi");
        println!("cargo::rustc-cfg=kanna_vt_abi");
    }
}

/// This checkout's libghostty-vt, or, in a worktree that hasn't built one, the main checkout's when it's the pinned commit's.
fn built_here_or_in_main_checkout(root: &Path, target: &str, pinned: &str) -> PathBuf {
    let here = root.join("target/ghostty-vt").join(target);
    if here.join("lib/libghostty-vt.a").exists() {
        return here;
    }
    let common = std::process::Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()));
    let main = common.and_then(|git| git.parent().map(|main| main.join("target/ghostty-vt").join(target)));
    match main {
        Some(main) if main != here && main.join("lib/libghostty-vt.a").exists() && fs::read_to_string(main.join("ghostty.rev")).is_ok_and(|rev| rev.trim() == pinned) => main,
        _ => here,
    }
}

fn pinned_commit(env_sh: &Path) -> String {
    let text = fs::read_to_string(env_sh).unwrap_or_else(|e| panic!("can't read {}: {e}", env_sh.display()));
    text.lines()
        .find_map(|line| line.trim().strip_prefix("KANNA_GHOSTTY_COMMIT="))
        .map(|value| value.trim_matches('"').to_string())
        .unwrap_or_else(|| panic!("no KANNA_GHOSTTY_COMMIT in {}", env_sh.display()))
}
