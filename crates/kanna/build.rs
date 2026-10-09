use std::path::PathBuf;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(kmux_reference)");
    println!("cargo::rerun-if-env-changed=KMUX_DIR");
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let kmux = std::env::var_os("KMUX_DIR").map(PathBuf::from).unwrap_or_else(|| manifest.join("../../../kmux"));
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let mut found = true;
    for name in ["commands", "output"] {
        let source = kmux.join(format!("crates/kmux/src/{name}.rs"));
        println!("cargo::rerun-if-changed={}", source.display());
        match std::fs::read_to_string(&source) {
            Ok(text) => {
                let body: String = text.lines().filter(|line| !line.starts_with("//!")).map(|line| format!("{line}\n")).collect();
                std::fs::write(out.join(format!("kmux_{name}.rs")), body).unwrap();
            }
            Err(_) => found = false,
        }
    }
    if found {
        println!("cargo::rustc-cfg=kmux_reference");
    }
    std::fs::write(out.join("kmux_dir.txt"), kmux.display().to_string()).unwrap();
}
