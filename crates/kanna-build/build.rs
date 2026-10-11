use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-env-changed=KANNA_CHANNEL");
    println!("cargo::rerun-if-changed=build.rs");
    let channel = std::env::var("KANNA_CHANNEL").ok().filter(|c| !c.is_empty()).unwrap_or_else(|| "dev".into());
    if !["production", "staging", "dev"].contains(&channel.as_str()) {
        panic!("KANNA_CHANNEL is \"{channel}\": use production, staging or dev");
    }
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let checkout = manifest.join("../..").canonicalize().unwrap_or_else(|_| manifest.join("../.."));
    let dot_git = checkout.join(".git");
    println!("cargo::rerun-if-changed={}", dot_git.display());
    let worktree = if dot_git.is_file() { checkout.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() } else { String::new() };
    println!("cargo::rustc-env=KANNA_BUILD_CHANNEL={channel}");
    println!("cargo::rustc-env=KANNA_BUILD_CHECKOUT={}", checkout.display());
    println!("cargo::rustc-env=KANNA_BUILD_WORKTREE={worktree}");
}
