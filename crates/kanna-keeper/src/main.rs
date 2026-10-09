//! kanna-keeper: creates Kanna's ptys and holds them, so the processes in
//! them outlive a daemon restart. See docs/keeper.md.

use kanna_keeper::{paths, server};

const USAGE: &str = "usage: kanna-keeper [--state-dir DIR] [--detach]

Runs the keeper for DIR (default $KANNA_STATE_DIR, else Kanna's data
folder), listening on DIR/keeper.sock. With --detach it runs in the
background and this command returns once it's listening.";

fn usage() -> ! {
    eprintln!("{USAGE}");
    std::process::exit(2)
}

fn main() {
    let mut state_dir = None;
    let mut detach = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--state-dir" => state_dir = Some(args.next().unwrap_or_else(|| usage()).into()),
            "--detach" => detach = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            _ => usage(),
        }
    }
    let settings = server::Settings::from_env(state_dir.unwrap_or_else(paths::default_state_dir));
    let result = if detach { server::detach(settings) } else { server::run(settings, None) };
    if let Err(e) = result {
        eprintln!("kanna-keeper: {e:#}");
        std::process::exit(1);
    }
}
