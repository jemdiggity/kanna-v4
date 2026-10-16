use kanna_diff::patch::{Fold, Kind, Status};
use kanna_diff::{git, ignored, load};
use std::collections::HashSet;
use std::fs;

fn repo(name: &str) -> String {
    let folder = std::env::temp_dir().join(format!("kanna-diff-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).unwrap();
    let folder = folder.canonicalize().unwrap().to_string_lossy().into_owned();
    let run = |args: &[&str]| {
        let mut all = vec!["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"];
        all.extend(args);
        git(&all, &folder, None).unwrap();
    };
    let write = |path: &str, text: &str| fs::write(format!("{folder}/{path}"), text).unwrap();
    run(&["init", "-q", "-b", "main"]);
    write("a.txt", "one\ntwo\nthree\n");
    write("Cargo.lock", "x\n");
    write(".gitignore", "target/\n");
    run(&["add", "."]);
    run(&["commit", "-qm", "first"]);
    run(&["checkout", "-qb", "task"]);
    write("a.txt", "one\n2\nthree\n");
    run(&["commit", "-qam", "second"]);
    write("a.txt", "one\n2\nthree\nfour\n");
    write("Cargo.lock", "x\ny\n");
    write("b.txt", "new\n");
    fs::create_dir_all(format!("{folder}/target")).unwrap();
    write("target/out", "built\n");
    folder
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn paths(diff: &kanna_diff::Diff) -> Vec<&str> {
    diff.files.iter().map(|f| f.path.as_str()).collect()
}

#[test]
fn the_tasks_changes_against_main_come_with_untracked_files_and_lock_files_folded() {
    let folder = repo("task");
    let diff = load(&folder, &args(&["--merge-base", "main"]), &HashSet::new());
    assert_eq!(diff.error, None);
    assert_eq!(diff.command(), "git diff --merge-base main");
    assert_eq!(diff.top, folder);
    assert_eq!(paths(&diff), ["Cargo.lock", "a.txt", "b.txt"]);
    assert_eq!(diff.files[0].fold, Some(Fold::LockFile));
    assert!(diff.files[0].hunks.is_empty());
    let changed: Vec<&str> = diff.files[1].hunks.iter().flat_map(|h| &h.lines).filter(|l| l.kind != Kind::Context).map(|l| l.text.as_str()).collect();
    assert_eq!(changed, ["two", "2", "four"]);
    assert!(diff.files[2].untracked);
    assert_eq!(diff.files[2].status, Status::Added);
    let shown = load(&folder, &args(&["--merge-base", "main"]), &HashSet::from(["Cargo.lock".to_string()]));
    assert_eq!((shown.files[0].fold.clone(), shown.files[0].added, shown.files[0].hunks.len()), (None, 1, 1));
}

#[test]
fn staged_and_two_commit_diffs_leave_untracked_files_out_and_pathspecs_narrow_both() {
    let folder = repo("staged");
    assert!(load(&folder, &args(&["--staged"]), &HashSet::new()).files.is_empty());
    assert_eq!(paths(&load(&folder, &args(&["main", "task"]), &HashSet::new())), ["a.txt"]);
    assert_eq!(paths(&load(&folder, &args(&["--", "b.txt"]), &HashSet::new())), ["b.txt"]);
    assert_eq!(paths(&load(&folder, &args(&[]), &HashSet::new())), ["Cargo.lock", "a.txt", "b.txt"]);
}

#[test]
fn gits_error_is_the_diff() {
    let folder = repo("error");
    let diff = load(&folder, &args(&["nope"]), &HashSet::new());
    assert!(diff.files.is_empty());
    assert!(diff.error.unwrap().contains("nope"));
    assert!(load("/", &args(&[]), &HashSet::new()).error.unwrap().contains("not a git repository"));
}

#[test]
fn ignored_paths_are_the_ones_git_ignores() {
    let folder = repo("ignored");
    assert_eq!(ignored(&args(&["target/out", "a.txt", "b.txt"]), &folder), HashSet::from(["target/out".to_string()]));
}
