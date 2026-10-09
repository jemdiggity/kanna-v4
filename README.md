# Kanna

Kanna is a terminal multiplexer for macOS whose terminals outlive the app. Quit or crash Kanna.app, or upgrade its daemon, and every shell and agent keeps running with its screen intact. It has windows, tabs and splits with terminal, web, markdown (with mermaid diagrams) and iOS Simulator panes, and the `kanna` CLI drives all of it.

[kmux](https://github.com/tampopogk/kmux) is the reference model: Kanna.app does what kmux does, the same way, and the `kanna` CLI matches kmux's command for command. See [docs/kanna-spec.md](docs/kanna-spec.md) for the design, [docs/plans/mux-build.md](docs/plans/mux-build.md) for how it was built, and [docs/roadmap.md](docs/roadmap.md) for what comes next.

```mermaid
flowchart TB
    CLI["kanna CLI, agents"] -->|socket| D
    APP["Kanna.app<br/>windows · tabs · splits · views"] <-->|app link| D["kanna-daemon<br/>terminals and their screens"]
    D <-->|socket| K["kanna-keeper<br/>holds the ptys"]
    K -->|creates| PTY["shells, agents"]
```

- **Kanna.app** (`apps/kanna`, Swift) draws the panes with Ghostty and owns the layout. It starts the daemon if none is running.
- **kanna-daemon** (`crates/kanna-daemon`, Rust) runs Kanna's mux core (`crates/kanna-mux`, a port of kmux's) and keeps every terminal's screen.
- **kanna-keeper** (`crates/kanna-keeper`, Rust) holds the ptys, so the programs survive the daemon restarting.
- **kanna** (`crates/kanna`, Rust) is the CLI.

## Build

Kanna builds against Ghostty from the `kanna4` branch of [jemdiggity/ghostty](https://github.com/jemdiggity/ghostty/tree/kanna4), which the scripts clone and pin (`scripts/env.sh`).

```sh
scripts/build-ghostty-vt.sh       # libghostty-vt for the daemon
cargo build --workspace --release # daemon, keeper, CLI
scripts/build-kanna-app.sh        # target/Kanna.app, with the daemon and keeper inside
```

The first build compiles Ghostty with Zig (downloaded if the right version isn't installed), which takes a while.

## Use

```sh
open target/Kanna.app
kanna open --name server --cmd "npm run dev" --split right --size 1/3
kanna open md docs/kanna-spec.md --tab
kanna list
kanna help
```

`kanna` starts Kanna.app if it isn't running. `--mux kmux` drives a kmux you run instead, and `--instance NAME` runs a separate Kanna with its own app, daemon and terminals.

## Test

Some tests check Kanna against kmux, the reference model, using kmux's source and tests in `references/kmux` (a git subtree).

```sh
cargo test --workspace
swift test --package-path apps/kanna -c release --scratch-path target/kanna-build
scripts/e2e-kanna.sh              # private instances on their own sockets; never touches yours
```

## License

[FSL-1.1-MIT](LICENSE): source-available; each release becomes MIT two years after it is published. kmux and Ghostty are MIT; [NOTICE](NOTICE) lists the code derived from them.
