# Build Channels

**Status:** built · **Builds on:** [instances](../kanna-spec.md#411-the-kanna-cli) · **Why:** Kanna is developed inside Kanna: every task builds and runs its own Kanna in its worktree while the Kanna it works in keeps running

A build of Kanna is one of three **channels**, chosen when it's built. Each channel is a separate app (its own name, bundle ID and folders), so all three run side by side; a **dev** build made in a git worktree is, in addition, an app of its own named after the worktree, so every task's Kanna runs beside the others and beside the one it was built from.

Channels and instances are for developing and testing Kanna, not a user feature: the app offers no way to start an instance, and users get one Kanna and work on another project in a Desk (File → New Desk, `kanna PATH`). Developers reach instances through dev builds, the e2e scripts and `kanna --instance`.

## 1. What each build is

| Build | App name | Bundle ID | Default instance | Its folder | CLI starts it with |
|---|---|---|---|---|---|
| `scripts/build.sh production` | Kanna | `dev.kanna.app` | `default` | `kanna-prod` | `open -b dev.kanna.app` (installed in /Applications) |
| `scripts/build.sh staging` | Kanna Staging | `dev.kanna.app.staging` | `default` | `kanna-staging` | `open -b dev.kanna.app.staging` |
| `scripts/build.sh` in the main checkout | Kanna Dev | `dev.kanna.app.dev` | `default` | `kanna-dev` | `open -a <checkout>/target/Kanna.app` |
| `scripts/build.sh` in `.kanna-worktrees/task-3f9a1c2e` | Kanna Dev task-3f9a1c2e | `dev.kanna.app.dev.task-3f9a1c2e` | `task-3f9a1c2e` | `kanna-dev-task-3f9a1c2e` | `open -a <worktree>/target/Kanna.app` |

The dev build in the main checkout is the Kanna that came before channels: same bundle ID, same `kanna-dev` folder, same terminals and layout.

```text
~/Library/Application Support/
├── kanna-prod/                  production's default instance
├── kanna-prod-work/             production's instance named work
├── kanna-staging/               staging's
├── kanna-dev/                   the main checkout's dev build
├── kanna-dev-task-3f9a1c2e/     the dev build of worktree task-3f9a1c2e
│   └── instance.json            {channel, name, bundle, checkout, worktree, app}, written by its daemon
└── kanna-dev-task-7c01d4aa/     another worktree's
```

Each folder is a whole Kanna (daemon, keeper, terminals, layout, backlog), so nothing is shared between them. Instance names are per channel: `--instance work` in a staging build is `kanna-staging-work`.

## 2. How a build knows what it is

```mermaid
flowchart LR
    S["scripts/build.sh [channel]"] -->|"KANNA_CHANNEL"| B["crates/kanna-build<br/>build.rs: channel; checkout;<br/>worktree when .git is a file"]
    B --> CLI["kanna<br/>default instance, app to start"]
    B --> D["kanna-daemon<br/>default socket, instance.json"]
    B -->|"target/release/kanna-build<br/>prints the identity"| P["build-kanna-app.sh<br/>Info.plist: CFBundleName,<br/>CFBundleIdentifier, KannaChannel,<br/>KannaInstance"]
    P --> A["Kanna.app<br/>default socket, Dock badge"]
```

- **One source of truth:** `crates/kanna-build` derives everything from the channel and the worktree's folder name (kept to letters, digits, `-` and `_`, 32 at most; the bundle ID lowercases it). `KANNA_CHANNEL` defaults to `dev`; a plain `cargo build` in a worktree makes that worktree's dev CLI.
- **The app** reads `KannaChannel` and `KannaInstance` from its Info.plist; staging and dev builds put a STAGING or DEV band on their Dock icon, and the Kanna menu's Quit item is named after the app.

## 3. Which Kanna a command reaches

```mermaid
flowchart TB
    Q{"--instance NAME?"} -->|yes| N["NAME in this build's channel"]
    Q -->|no| S{"$KANNA_SOCKET?"}
    S -->|"yes (any Kanna terminal)"| T["that terminal's Kanna, whatever its channel"]
    S -->|no| I{"$KANNA_INSTANCE?"}
    I -->|yes| N2["that name in this build's channel"]
    I -->|no| W["this build's default instance<br/>(the worktree's, in a worktree)"]
```

So in production's terminals every `kanna` talks to production; to reach the worktree's own Kanna from there, use `scripts/run-kanna.sh`, which clears `KANNA_SOCKET`, `KANNA_INSTANCE` and `KANNA_PANE`.

## 4. Running and cleaning up

| Command | Does |
|---|---|
| `scripts/build.sh [production\|staging\|dev] [--arch arm64\|x86_64]` | builds everything as that channel (default `dev`) for that architecture (default this Mac's, [§5](#5-architectures)); `scripts/build-kanna-app.sh` takes the same arguments. Dev builds the Swift app in debug, the others in release; a worktree starts from the main checkout's builds ([build speed](build-speed.md)) |
| `scripts/run-kanna.sh` | opens this checkout's `target/Kanna.app` |
| `scripts/run-kanna.sh COMMAND …` | runs this checkout's CLI against this checkout's Kanna (starting it if needed), from anywhere, a production terminal included |
| `kanna instances` | the running instances of every channel: name, channel, windows and panes, and the worktree a dev build came from (`(gone)` once it's removed) |
| `kanna instances prune` | deletes the folders of instances whose worktree is gone, keeping any that still answer or whose keeper still runs |
| `kanna --version` | the CLI's version and which app it is, e.g. `kanna 0.1.0 (Kanna Dev task-3f9a1c2e)` |

Production is the build that goes in /Applications (`ditto target/Kanna.app /Applications/Kanna.app`); its folder starts empty. To carry the main checkout's dev Kanna over (layout, backlog, terminals), quit it, stop its keeper (`kill $(cat ~/Library/Application\ Support/kanna-dev/keeper.pid)`, which ends its terminals) and move `kanna-dev` to `kanna-prod`.

## 5. Architectures

`--arch` builds a whole Kanna.app for one architecture: the Swift app (`swift build --arch`), the Rust helpers (`cargo --target`) and libghostty-vt (Zig). Each architecture gets its own app, never a universal binary.

| `--arch` on an arm64 Mac | Rust | Swift build | libghostty-vt | App |
|---|---|---|---|---|
| left out, or `arm64` | `target/release` | `target/kanna-build` | `target/ghostty-vt/aarch64-apple-darwin` | `target/Kanna.app` |
| `x86_64` | `target/x86_64-apple-darwin/release` | `target/x86_64-apple-darwin/kanna-build` | `target/ghostty-vt/x86_64-apple-darwin` | `target/x86_64-apple-darwin/Kanna.app` |

- **The native build stays where it was;** another architecture's goes in `target/<rust triple>/`, beside cargo's own output for it, so building one never cleans or overwrites the other.
- **GhosttyKit is shared:** Ghostty's xcframework already holds arm64 and x86_64, and SwiftPM links the slice it needs.
- **The identity comes from the host:** the Info.plist is written from `target/release/kanna-build`, built for this Mac, since the other architecture's binaries may not run here.
- **Borrowing in a worktree is per architecture:** cargo's dependencies come from the main checkout's `target/<triple>/release` when it has one; Ghostty and the Swift packages are borrowed as for a native build ([build speed](build-speed.md)).
- **The Rust target** is added with `rustup target add` the first time it's needed.
- **A non-native dev build** is for checking that it builds: its CLI still opens `target/Kanna.app`.

## 6. Open questions

- **Pruning on `task done`:** the daemon removes a task's worktree; it could also prune that worktree's dev instance then, rather than waiting for `kanna instances prune`.
- **The older apps' names:** `/Applications/Kanna.app` and `Kanna Staging.app` may still be Kanna v3 (`build.kanna`, `build.kanna.staging`, folder `Kanna`); channels use different bundle IDs and folders, so installing production replaces only the file.
