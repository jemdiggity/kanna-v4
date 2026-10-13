# The Daemon Follows the App's Build

**Status:** built · **Builds on:** [the app link](../kanna-spec.md#45-the-app-link), [restarting the daemon](../kanna-spec.md#48-restarting-the-daemon) · **Why:** quitting the app leaves the daemon running, so a rebuilt app used to attach to the old daemon and run against its old commands until someone killed it by hand

A relaunched app whose bundled daemon differs from the one running replaces it. Nothing is lost: the keeper holds the ptys, and the next daemon adopts them.

## 1. The build ID

The **build ID** is the SHA-256 of the `kanna-daemon` binary, in hex.

| Who | Hashes | When |
|---|---|---|
| the daemon | its own executable | at start, before a rebuild can replace the file |
| the app | `Contents/Helpers/kanna-daemon`, the daemon it would start | once, at its first hello |
| the CLI | the `kanna-daemon` beside it (in the bundle, or `target/release`) | only when a command fails |

- **Why the binary and not the version and commit:** a dev rebuild with uncommitted changes has the same commit and different code, and a commit that doesn't touch the daemon needn't replace it. The hash changes exactly when the daemon does; an app-only rebuild keeps its daemon.
- **Why not a stamp per build:** every rebuild would replace a daemon that hadn't changed.

## 2. Where it lives

Layers numbered as in [4.1](../kanna-spec.md#41-layers-and-processes); 1 and 2 take no part.

```mermaid
flowchart TB
    subgraph CLIP["CLI process"]
        CLI["kanna CLI<br/>on a failed command:<br/>compares the builds"]
    end
    subgraph APP["Kanna.app process"]
        L3["3 · Mux UI<br/>DaemonLink: hello {build}<br/>DaemonStarter:<br/>starts its daemon"]
    end
    subgraph DAEMON["daemon process"]
        L4["4 · kanna-mux<br/>knows its own build<br/>stops for another one"]
    end
    subgraph KEEPER["keeper process"]
        L5["5 · Keeper<br/>holds the ptys"]
    end
    subgraph OS["6 · macOS"]
        BIN["kanna-daemon binary<br/>Helpers/ or target/release"]
        PTY["ptys<br/>shells, agents"]
    end
    CLI -->|"socket: capabilities"| L4
    L3 <-->|"socket: app.hello"| L4
    L4 <-->|"socket: adopt"| L5
    L5 -->|"creates"| PTY
    L4 <-->|"reads, writes"| PTY
    CLI -.->|"hashes the one beside it"| BIN
    L3 -.->|"hashes, starts"| BIN
    L4 -.->|"hashed its own at start"| BIN
    classDef kanna fill:#2f6f4f,stroke:#6fcf97,color:#ffffff
    classDef kmux fill:#2f4f8f,stroke:#7fa7ff,color:#ffffff
    classDef os fill:#4a4a55,stroke:#9a9aa8,color:#ffffff
    classDef ext fill:#6b4f2f,stroke:#e0a960,color:#ffffff
    class L5 kanna
    class L3,L4 kmux
    class BIN,PTY os
    class CLI ext
```

Green: Kanna's code · blue: kmux's, as Kanna's · grey: macOS · brown: clients. Dashed: reads a file.

| Layer | What it does with the build | Process |
|---|---|---|
| CLI (a client) | hashes the `kanna-daemon` beside it and asks the daemon's (`capabilities`), only after a command fails | CLI |
| 3 · Mux UI | hashes `Helpers/kanna-daemon`, sends it in `app.hello`; on `other_build` starts that daemon at once | app |
| 4 · kanna-mux | hashes its own binary at start; refuses a hello of another build when no app is attached, and stops | daemon |
| 5 · Keeper | nothing: it keeps the ptys while one daemon stops and the next adopts them | keeper |
| 6 · macOS | the binary on disk, and the ptys | — |

## 3. Hello from another build

```mermaid
sequenceDiagram
    participant A as rebuilt app
    participant D1 as old daemon
    participant K as keeper
    participant D2 as new daemon
    A->>D1: app.hello {pid, build: B2, panes}
    Note over D1: no app attached, its build is B1
    D1->>A: ok: false, other_build
    Note over D1: stops as on SIGTERM:<br/>saves terminals.json and the screens
    D1--xA: link closes
    Note over A: no 2 s grace: starts Helpers/kanna-daemon<br/>as soon as nothing answers
    A->>D2: spawn (waits for daemon.lock)
    D2->>K: adopt (same ptys, same pids)
    A->>D2: app.hello {build: B2}
    D2->>A: ok, daemon {pid, build: B2}, panes (the same IDs)
```

- **Only with no app attached.** A second app gets `app_attached` as before, whatever its build, so two running apps of different builds can't take turns stopping each other's daemon.
- **A hello without `build`** (an older app, the test apps) is accepted, as before.
- **The app** answers `other_build` by retrying at once rather than backing off, and starts its daemon without the grace it gives a daemon restarted on purpose; at most one start every 10 s still holds, so an app whose bundle changed under it can't loop.

## 4. The CLI meets another build

The CLI never stops a daemon: the app attached to it may be the old build too. It talks to it as before, and when a command fails it asks the daemon its build (`capabilities` answers `build`) and, if it differs from its own, adds a line:

```text
$ kanna send nosuch x
kanna: no pane "nosuch" [not_found]
(the daemon on this socket is from another build than this kanna (a4cb40a16617 here, c9e6b91e5a5d there); quit and relaunch Kanna.app to replace it)
```

## 5. Checked by

`crates/kanna-daemon/tests/restart.rs` (a hello of the same build is accepted with the build in the reply; one of another build is refused with `other_build`, the daemon stops, the shell lives, and the next daemon has it with the same ID and pid), `crates/kanna-build` (the ID is the binary's SHA-256), and S61 of `scripts/e2e-kanna.sh`: a daemon re-signed as an older build, a terminal opened on it, the CLI's note; the app launched, the old daemon stops, the app's daemon takes over with the terminal's pid and screen, and the note is gone.
