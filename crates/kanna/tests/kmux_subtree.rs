use std::path::Path;
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git").current_dir(root).args(args).output().expect("git runs");
    assert!(out.status.success(), "git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

#[test]
fn references_kmux_is_the_kmux_it_was_last_pulled_or_pushed_as() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let squash = git(&root, &["log", "-1", "--format=%H", "--grep=^Squashed 'references/kmux/'", "HEAD"]);
    assert!(!squash.is_empty(), "no subtree pull of references/kmux in HEAD's history");
    let kmux = git(&root, &["rev-parse", &format!("{squash}^{{tree}}")]);
    let here = git(&root, &["rev-parse", "HEAD:references/kmux"]);
    assert_eq!(
        here,
        kmux,
        "references/kmux has changes kmux doesn't (docs/specs/kmux-subtree.md): push them to a kmux branch with\n  \
         git subtree push --prefix=references/kmux --rejoin --squash https://github.com/tampopogk/kmux.git BRANCH\n\
         and open a PR there; changed: {}",
        git(&root, &["diff", "--stat", &squash, "HEAD:references/kmux", "--"]).lines().last().unwrap_or("")
    );
}
