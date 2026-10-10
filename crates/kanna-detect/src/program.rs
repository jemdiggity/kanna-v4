use crate::rules::{Agent, RuleSet};
use std::collections::HashMap;
use std::os::fd::RawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, UNIX_EPOCH};

const INTERPRETERS: &[&str] = &["node", "bun", "deno", "python", "python3", "ruby", "perl", "sh", "bash", "zsh", "dash", "fish"];
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Process {
    pub pid: i32,
    pub executable: Option<PathBuf>,
    pub argv: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    pub agent: String,
    pub pid: i32,
    pub command: Vec<String>,
}

pub fn foreground_group(fd: RawFd) -> Option<i32> {
    let group = unsafe { libc::tcgetpgrp(fd) };
    (group > 0).then_some(group)
}

pub fn identify(group: i32, rules: &RuleSet) -> Option<Program> {
    let mut members = group_members(group);
    members.sort_by_key(|pid| (*pid != group, *pid));
    members.into_iter().filter_map(process).find_map(|p| recognise(&p, rules))
}

pub fn recognise(process: &Process, rules: &RuleSet) -> Option<Program> {
    let base = |p: &str| Path::new(p).file_name().and_then(|n| n.to_str()).map(str::to_owned);
    let argv0 = process.argv.first().and_then(|a| base(a));
    let exe = process.executable.as_ref().and_then(|p| p.to_str()).and_then(base);
    let names = [argv0.clone(), exe.clone()];
    let direct = names.iter().flatten().find_map(|name| rules.agent_for_program(name));
    if let Some(agent) = direct {
        if is_kanna(exe.as_deref()) {
            return None;
        }
        let executable = process.executable.clone().map(|p| p.to_string_lossy().into_owned()).or_else(|| process.argv.first().cloned())?;
        return Some(Program { agent: agent.name().into(), pid: process.pid, command: vec![executable] });
    }
    let interpreter = names.iter().flatten().any(|n| INTERPRETERS.contains(&n.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.')));
    if !interpreter {
        return None;
    }
    let (index, script) = process.argv.iter().enumerate().skip(1).take_while(|(_, a)| *a != "-c" && *a != "-e").find(|(_, a)| !a.starts_with('-'))?;
    let stem = Path::new(script).file_stem()?.to_str()?;
    let agent = rules.agent_for_program(stem)?;
    let runner = process.executable.clone().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| process.argv[0].clone());
    let mut command = vec![runner];
    command.extend(process.argv[1..=index].iter().cloned());
    Some(Program { agent: agent.name().into(), pid: process.pid, command })
}

fn is_kanna(name: Option<&str>) -> bool {
    name.is_some_and(|n| n.starts_with("kanna"))
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ProbeKey {
    command: Vec<String>,
    modified: u128,
}

enum Probed {
    Running,
    Done(Option<String>),
}

fn probes() -> &'static Mutex<HashMap<ProbeKey, Probed>> {
    static PROBES: OnceLock<Mutex<HashMap<ProbeKey, Probed>>> = OnceLock::new();
    PROBES.get_or_init(Mutex::default)
}

pub enum Version {
    Known(Option<String>),
    Pending,
}

pub fn version(program: &Program, agent: &Agent) -> Version {
    let Some(probe) = &agent.file.version_probe else { return Version::Known(None) };
    let Some(pattern) = agent.probe_pattern().cloned() else { return Version::Known(None) };
    let binary = program.command.last().cloned().unwrap_or_default();
    let modified = std::fs::metadata(&binary).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
    let key = ProbeKey { command: program.command.clone(), modified };
    let mut table = probes().lock().unwrap_or_else(|e| e.into_inner());
    match table.get(&key) {
        Some(Probed::Done(found)) => return Version::Known(found.clone()),
        Some(Probed::Running) => return Version::Pending,
        None => {}
    }
    table.insert(key.clone(), Probed::Running);
    drop(table);
    let mut argv = program.command.clone();
    argv.extend(probe.args.iter().cloned());
    std::thread::Builder::new()
        .name("kanna.probe".into())
        .spawn(move || {
            let found = run(&argv, PROBE_TIMEOUT).and_then(|text| pattern.captures(&text).and_then(|c| c.get(1).or_else(|| c.get(0))).map(|m| m.as_str().to_owned()));
            probes().lock().unwrap_or_else(|e| e.into_inner()).insert(key, Probed::Done(found));
        })
        .map(|_| Version::Pending)
        .unwrap_or(Version::Known(None))
}

pub fn run(argv: &[String], timeout: Duration) -> Option<String> {
    let mut child = Command::new(argv.first()?).args(&argv[1..]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().ok()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let out = child.wait_with_output().ok()?;
                return Some(format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)));
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn group_members(group: i32) -> Vec<i32> {
    let mut pids = vec![0i32; 256];
    let bytes = unsafe { libc::proc_listpgrppids(group, pids.as_mut_ptr().cast(), (pids.len() * std::mem::size_of::<i32>()) as libc::c_int) };
    if bytes <= 0 {
        return vec![group];
    }
    pids.truncate(bytes as usize / std::mem::size_of::<i32>());
    pids.retain(|pid| *pid > 0);
    if !pids.contains(&group) {
        pids.push(group);
    }
    pids
}

#[cfg(target_os = "macos")]
fn process(pid: i32) -> Option<Process> {
    let mut path = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let n = unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), path.len() as u32) };
    let executable = (n > 0).then(|| PathBuf::from(String::from_utf8_lossy(&path[..n as usize]).into_owned()));
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut size: libc::size_t = 0;
    let mut argv = vec![];
    if unsafe { libc::sysctl(mib.as_mut_ptr(), 3, std::ptr::null_mut(), &mut size, std::ptr::null_mut(), 0) } == 0 && size > 4 {
        let mut buffer = vec![0u8; size];
        if unsafe { libc::sysctl(mib.as_mut_ptr(), 3, buffer.as_mut_ptr().cast(), &mut size, std::ptr::null_mut(), 0) } == 0 {
            buffer.truncate(size);
            argv = procargs(&buffer);
        }
    }
    (executable.is_some() || !argv.is_empty()).then_some(Process { pid, executable, argv })
}

#[cfg(target_os = "macos")]
fn procargs(buffer: &[u8]) -> Vec<String> {
    if buffer.len() < 4 {
        return vec![];
    }
    let argc = i32::from_ne_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]).max(0) as usize;
    let mut rest = &buffer[4..];
    let path_end = rest.iter().position(|b| *b == 0).unwrap_or(rest.len());
    rest = &rest[path_end..];
    let start = rest.iter().position(|b| *b != 0).unwrap_or(rest.len());
    rest = &rest[start..];
    rest.split(|b| *b == 0).take(argc).map(|a| String::from_utf8_lossy(a).into_owned()).collect()
}

#[cfg(target_os = "linux")]
fn group_members(group: i32) -> Vec<i32> {
    let mut pids: Vec<i32> = std::fs::read_dir("/proc")
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.file_name().to_str()?.parse::<i32>().ok())
                .filter(|pid| {
                    std::fs::read_to_string(format!("/proc/{pid}/stat"))
                        .ok()
                        .and_then(|stat| stat.rsplit_once(')').map(|(_, rest)| rest.split_whitespace().nth(2).and_then(|g| g.parse::<i32>().ok())))
                        .flatten()
                        == Some(group)
                })
                .collect()
        })
        .unwrap_or_default();
    if !pids.contains(&group) {
        pids.push(group);
    }
    pids
}

#[cfg(target_os = "linux")]
fn process(pid: i32) -> Option<Process> {
    let executable = std::fs::read_link(format!("/proc/{pid}/exe")).ok();
    let argv: Vec<String> = std::fs::read(format!("/proc/{pid}/cmdline"))
        .map(|raw| raw.split(|b| *b == 0).filter(|a| !a.is_empty()).map(|a| String::from_utf8_lossy(a).into_owned()).collect())
        .unwrap_or_default();
    (executable.is_some() || !argv.is_empty()).then_some(Process { pid, executable, argv })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn group_members(group: i32) -> Vec<i32> {
    vec![group]
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn process(_pid: i32) -> Option<Process> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{RuleSet, Source};

    fn rules() -> RuleSet {
        let mut set = RuleSet::default();
        for (agent, programs) in [("claude", r#"["claude"]"#), ("codex", r#"["codex"]"#), ("mock", r#"["mock-agent"]"#)] {
            let text = format!(r#"{{"schema":1,"agent":"{agent}","version":"1","engine":1,"programs":{programs},"rules":[]}}"#);
            set.insert(Agent::parse(&text, Source::Bundled).unwrap());
        }
        set
    }

    fn proc(executable: &str, argv: &[&str]) -> Process {
        Process { pid: 7, executable: Some(executable.into()), argv: argv.iter().map(|s| s.to_string()).collect() }
    }

    #[test]
    fn agents_are_known_by_their_name_their_binary_or_their_script() {
        let rules = rules();
        let found = |p: Process| recognise(&p, &rules).map(|p| (p.agent, p.command));
        assert_eq!(found(proc("/Users/x/.local/share/claude/versions/2.1.296", &["claude"])), Some(("claude".into(), vec!["/Users/x/.local/share/claude/versions/2.1.296".into()])));
        assert_eq!(found(proc("/opt/vendor/codex/codex", &["/opt/vendor/codex/codex", "--yolo"])).map(|f| f.0), Some("codex".into()));
        assert_eq!(
            found(proc("/usr/local/bin/node", &["node", "/n/lib/node_modules/@openai/codex/bin/codex.js", "resume"])),
            Some(("codex".into(), vec!["/usr/local/bin/node".into(), "/n/lib/node_modules/@openai/codex/bin/codex.js".into()]))
        );
        assert_eq!(found(proc("/usr/bin/python3", &["/usr/bin/python3", "/t/mock-agent"])).map(|f| f.0), Some("mock".into()));
        assert_eq!(found(proc("/bin/zsh", &["-zsh"])), None);
        assert_eq!(found(proc("/bin/zsh", &["zsh", "-l"])), None);
        assert_eq!(found(proc("/bin/zsh", &["zsh", "-l", "-c", "/t/mock-agent"])), None);
        assert_eq!(found(proc("/usr/bin/vim", &["vim", "claude.md"])), None);
    }

    #[test]
    fn a_probe_that_never_answers_is_stopped() {
        let started = Instant::now();
        assert_eq!(run(&["/bin/sleep".into(), "30".into()], Duration::from_millis(200)), None);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(run(&["/bin/echo".into(), "codex-cli 1.2.3".into()], Duration::from_secs(2)).unwrap().contains("1.2.3"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn this_process_reads_its_own_arguments() {
        let me = process(std::process::id() as i32).unwrap();
        assert!(me.executable.is_some());
        assert!(!me.argv.is_empty());
    }
}
