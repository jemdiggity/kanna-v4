use crate::rules::RuleSet;
use std::os::fd::RawFd;
use std::path::{Path, PathBuf};

pub const PROGRAMS: &[(&str, &str)] =
    &[("claude", "claude"), ("codex", "codex"), ("opencode", "opencode"), ("copilot", "copilot"), ("agy", "antigravity"), ("antigravity", "antigravity"), ("mock-agent", "mock")];

const INTERPRETERS: &[&str] = &["node", "bun", "deno", "python", "python3", "ruby", "perl", "sh", "bash", "zsh", "dash", "fish"];

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
    pub executable: Option<PathBuf>,
}

pub fn known_agent(program: &str) -> Option<&'static str> {
    PROGRAMS.iter().find(|(name, _)| *name == program).map(|(_, agent)| *agent)
}

pub fn known_agents() -> Vec<&'static str> {
    let mut agents: Vec<&str> = PROGRAMS.iter().map(|(_, a)| *a).collect();
    agents.dedup();
    agents
}

pub fn foreground_group(fd: RawFd) -> Option<i32> {
    let group = unsafe { libc::tcgetpgrp(fd) };
    (group > 0).then_some(group)
}

pub fn identify(group: i32, rules: &RuleSet) -> Option<Program> {
    let mut members = group_members(group);
    members.sort_by_key(|pid| (*pid != group, *pid));
    let processes: Vec<Process> = members.into_iter().filter_map(process).collect();
    let found = processes.iter().filter_map(recognise).find(|p| p.executable.is_some()).or_else(|| processes.iter().find_map(recognise))?;
    rules.agent(&found.agent).is_some().then_some(found)
}

pub fn recognise(process: &Process) -> Option<Program> {
    let base = |p: &str| Path::new(p).file_name().and_then(|n| n.to_str()).map(str::to_owned);
    let argv0 = process.argv.first().and_then(|a| base(a));
    let exe = process.executable.as_ref().and_then(|p| p.to_str()).and_then(base);
    if exe.as_deref().is_some_and(|n| n.starts_with("kanna")) {
        return None;
    }
    let names = [argv0, exe];
    if let Some(agent) = names.iter().flatten().find_map(|name| known_agent(name)) {
        let executable = process.executable.clone().filter(|p| p.is_absolute());
        return Some(Program { agent: agent.into(), pid: process.pid, executable });
    }
    let interpreter = names.iter().flatten().any(|n| INTERPRETERS.contains(&n.to_ascii_lowercase().trim_end_matches(|c: char| c.is_ascii_digit() || c == '.')));
    if !interpreter {
        return None;
    }
    let script = process.argv.iter().skip(1).take_while(|a| *a != "-c" && *a != "-e").find(|a| !a.starts_with('-'))?;
    let agent = known_agent(Path::new(script).file_stem()?.to_str()?)?;
    Some(Program { agent: agent.into(), pid: process.pid, executable: None })
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

    fn proc(executable: &str, argv: &[&str]) -> Process {
        Process { pid: 7, executable: Some(executable.into()), argv: argv.iter().map(|s| s.to_string()).collect() }
    }

    #[test]
    fn agents_are_known_by_their_name_their_binary_or_their_script() {
        let found = |p: Process| recognise(&p).map(|p| (p.agent, p.executable));
        assert_eq!(
            found(proc("/Users/x/.local/share/claude/versions/2.1.296", &["claude"])),
            Some(("claude".into(), Some("/Users/x/.local/share/claude/versions/2.1.296".into())))
        );
        assert_eq!(found(proc("/opt/vendor/codex/codex", &["/opt/vendor/codex/codex", "--yolo"])).map(|f| f.0), Some("codex".into()));
        assert_eq!(found(proc("/usr/local/bin/node", &["node", "/n/lib/node_modules/@openai/codex/bin/codex.js", "resume"])), Some(("codex".into(), None)));
        assert_eq!(found(proc("/usr/bin/python3", &["/usr/bin/python3", "/t/mock-agent"])), Some(("mock".into(), None)));
        assert_eq!(found(proc("/bin/zsh", &["-zsh"])), None);
        assert_eq!(found(proc("/bin/zsh", &["zsh", "-l"])), None);
        assert_eq!(found(proc("/bin/zsh", &["zsh", "-l", "-c", "/t/mock-agent"])), None);
        assert_eq!(found(proc("/usr/bin/vim", &["vim", "claude.md"])), None);
        assert_eq!(found(proc("/x/kanna-keeper", &["claude"])), None);
        assert_eq!(found(proc("/usr/bin/evil", &["evil"])), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn this_process_reads_its_own_arguments() {
        let me = process(std::process::id() as i32).unwrap();
        assert!(me.executable.is_some());
        assert!(!me.argv.is_empty());
    }
}
