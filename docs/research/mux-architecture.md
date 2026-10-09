# Kanna's mux: architecture

> 2026-10-09. Proposed. kmux stays an app and also ships libraries. Kanna builds its daemon, keeper and desktop app from two of kmux's layers, the **CLI shape** and the **mux UI**, and its terminals live in its daemon so they survive the app quitting, the way kanna-v3 did.

## Contents

1. [In short](#1-in-short)
2. [kmux: an app and its libraries](#2-kmux-an-app-and-its-libraries)
3. [Kanna: what it builds from kmux](#3-kanna-what-it-builds-from-kmux)
4. [Sessions and arrangement](#4-sessions-and-arrangement)
5. [Web and iOS panes](#5-web-and-ios-panes)
6. [How the kanna CLI fits](#6-how-the-kanna-cli-fits)
7. [The repos](#7-the-repos)
8. [Typing and output](#8-typing-and-output)
9. [Restarting the app](#9-restarting-the-app)
10. [Restarting the daemon](#10-restarting-the-daemon)
11. [Compared with kanna-v3](#11-compared-with-kanna-v3)
12. [Build order](#12-build-order)
13. [Glossary](#13-glossary)

---

## 1. In short

| | kmux | Kanna |
|---|---|---|
| What it is | an app, plus libraries | a desktop app, a daemon, a keeper and a CLI |
| Processes | one: the app | three: the app, the daemon, the keeper |
| Terminals live in | the app (Ghostty's own ptys) | the daemon (ptys held by the keeper) |
| After a restart | the layout comes back, terminals are new | everything comes back exactly |
| Ghostty | upstream | the fork (external I/O) |

**Kanna uses two layers of kmux:**
1. **The CLI shape:** the protocol and the sessions model (`open`, `list`, `send`, `navigate`, `close`, …). Kanna's daemon runs it, so `kanna` and `kmux` look alike.
2. **The mux UI:** the pane views (terminal, web, markdown, iOS) and, if wanted, the split and tab views. The Kanna app draws with them.

---

## 2. kmux: an app and its libraries

```mermaid
flowchart TB
    subgraph APP["kmux app (one process)"]
        KUI["kmux's windows, tabs, splits"]
        subgraph LIBS["kmux libraries"]
            CORE["CLI shape<br/>protocol, sessions model"]
            UI["mux UI<br/>pane views, split and tab views"]
        end
        PTYB["terminal backend: pty<br/>upstream Ghostty"]
        WEB["web views<br/>WebKit"]
        SIMV["iOS screen<br/>SimulatorKit"]
        KUI --> UI
        KUI --> CORE
        UI --> PTYB
        UI --> WEB
        UI --> SIMV
    end
    CLI["kmux CLI, agents"] -->|"socket"| CORE
    PTYB --- SH["shells"]
    SIMV <-->|"screen, touches"| SIM["iOS Simulator<br/>(its own processes)"]
```

| Library | Layer | What's in it |
|---|---|---|
| **KmuxCore** | CLI shape | the protocol, the sessions model and command handling; no UI |
| **KmuxUI** | mux UI | the pane views, the split and tab views, and the `TerminalBackend` seam |
| **KmuxClient** | CLI shape | the Rust client the CLIs use (it exists today as `kmux-client`) |

kmux itself doesn't change shape: one app, terminals with their own ptys, the same CLI. The work is drawing clean lines between these libraries and the app around them, so another app can take them. kmux has **no daemon**.

---

## 3. Kanna: what it builds from kmux

```mermaid
flowchart TB
    subgraph APP["Kanna desktop app (process)"]
        subgraph L1["UI layer (Kanna)"]
            DESK["Desk and Shelf<br/>arrangement"]
            REVIEW["markdown review"]
            EDIT["code editor"]
        end
        subgraph L2["Kanna layer"]
            TASKS["tasks, agents, worktrees, hooks"]
        end
        subgraph L3["kmux mux UI"]
            PANES["pane views"]
            EXT["terminal backend: external I/O<br/>(Kanna's, forked Ghostty)"]
            WEB["web views<br/>WebKit"]
            SIMV["iOS screen<br/>SimulatorKit"]
        end
        L1 --> L2
        L1 --> L3
        PANES --> EXT
        PANES --> WEB
        PANES --> SIMV
    end
    subgraph DP["Kanna daemon (process)"]
        CORE["kmux CLI shape<br/>protocol, sessions"]
        VT["a libghostty-vt screen per terminal"]
        SIMC["iOS control<br/>simctl, touches, accessibility"]
    end
    SIM["iOS Simulator<br/>(its own processes)"]
    subgraph KP["keeper (process)"]
        KEEPER["holds the ptys"]
    end
    CLI["kanna CLI, agents"]
    PTY["ptys"]
    SH["shells and agents"]
    L2 -->|"socket"| CORE
    EXT <-->|"terminal output and input"| CORE
    CLI -->|"socket"| CORE
    CORE <-->|"socket: start a shell,<br/>pty handles back"| KEEPER
    KEEPER -->|"creates, holds a copy"| PTY
    CORE <-->|"reads and writes"| PTY
    PTY --- SH
    SIMV <-->|"screen, your touches"| SIM
    SIMC <-->|"boot, launch, agent taps"| SIM
    CORE -->|"web commands forwarded"| WEB
```

| Kanna piece | Built from |
|---|---|
| **Kanna daemon** | kmux's CLI shape (KmuxCore) + a libghostty-vt screen per terminal + snapshots on disk + iOS control |
| **Keeper** | the `feat/keeper` branch and v3's keeper |
| **Kanna app** | Kanna's UI and Kanna layer + kmux's mux UI (KmuxUI) + Kanna's external-I/O terminal backend |
| **`kanna` CLI** | kmux's client (kmux-client), pointed at Kanna's daemon |

The keeper creates each pty and keeps a copy, so shells outlive a daemon restart; the daemon reads and writes them directly.

---

## 4. Sessions and arrangement

```mermaid
flowchart LR
    subgraph D["Kanna daemon: sessions"]
        S1["s1 terminal<br/>pty, screen, cwd, exit code"]
        S2["s2 web: URL"]
        S3["s3 markdown: file"]
        S4["s4 iOS: device, app"]
    end
    subgraph K["Kanna app: arrangement"]
        DESK["Desk and Shelf<br/>s1 and s3 on the desk,<br/>s2 on the shelf"]
    end
    DESK -. "by ID" .-> S1
    DESK -.-> S2
    DESK -.-> S3
    DESK -.-> S4
```

| | What it is | Owned and saved by |
|---|---|---|
| **Session** | a terminal (pty, screen, scrollback, cwd, exit code), or a pane's content (a web URL, a markdown file, an iOS app) | Kanna's daemon |
| **Arrangement** | where sessions appear | the Kanna app (Desk and Shelf) |

In the kmux app both live in the one process, as today: the sessions model and its windows, tabs and splits.

---

## 5. Web and iOS panes

Terminals can move into the daemon because a terminal is just bytes. Web pages and the iOS Simulator aren't, so each lives where it can.

```mermaid
flowchart LR
    subgraph APP["Kanna app"]
        WV["web view<br/>the live page"]
        SV["iOS pane<br/>draws the device screen"]
    end
    subgraph D["Kanna daemon"]
        WS["web session<br/>URL, history"]
        IS["iOS session<br/>device, app"]
        IC["iOS control"]
    end
    SIM["iOS Simulator device<br/>(CoreSimulator's own processes)"]
    WS -. "app opens it at the URL" .-> WV
    D -->|"web.eval, web.click, page text"| WV
    SV <-->|"screen, your touches"| SIM
    IC <-->|"boot, launch, taps,<br/>accessibility tree, screenshot"| SIM
    IS --- IC
```

| | Web | iOS |
|---|---|---|
| The live thing | a WebKit view, which must be in the app (it can't be drawn from another process) | the simulator device, already its own processes; it outlives both the app and the daemon |
| The daemon keeps | URL and history | device, app, and agent control (`tap`, `swipe`, `type`, accessibility tree, screenshots) |
| The app keeps | the web view itself | a view of the device's screen, plus your mouse as touches |
| App quits | the page reloads at its URL on relaunch; cookies and logins stay (on disk) | nothing is lost; the device keeps running |
| Agents with the app closed | web commands fail clearly (or the CLI starts the app) | everything works |

Pixels never go through the daemon: the iOS pane reads the screen straight from the simulator, as kmux's does today.

---

## 6. How the kanna CLI fits

```mermaid
flowchart TB
    CLI["kanna CLI, agents, hooks"]
    D["Kanna daemon"]
    S["sessions<br/>answered by the daemon"]
    A["the Kanna app<br/>arrangement and drawing"]
    CLI -->|"one socket"| D
    D --> S
    D -->|"forwards"| A
```

| Command | Touches | Answered by | With the app closed |
|---|---|---|---|
| `list`, `send`, `read`, `wait`, `close`, `restart` | sessions | daemon | works |
| `navigate`, `snapshot --text` | session content | daemon | works |
| iOS `tap`, `swipe`, `type`, accessibility tree, screenshots | the simulator | daemon | works |
| `open term --cmd …` | creates a session | daemon | works: created, shown when the app opens |
| `open … --split right`, `arrange`, `focus` | arrangement | forwarded to the app | fails clearly, or the CLI starts the app first |
| pixel snapshots of terminals, all `web.*` commands, web page text | the live view | forwarded to the app | fails clearly, or the CLI starts the app first |
| later: `kanna task …`, "put this on the desk" | the Kanna layer | the app | needs the app |

The `kmux` CLI is unchanged: it talks to the kmux app, which answers everything itself.

---

## 7. The repos

```mermaid
flowchart LR
    GU["ghostty-org/ghostty<br/>upstream"] -->|"pinned"| KMUX["tampopogk/kmux<br/>app + libraries, MIT"]
    GU -->|"rebased"| GF["jemdiggity/ghostty<br/>kmux-external-io"]
    KMUX -->|"libraries"| KV["jemdiggity/kanna-v4<br/>app, daemon, keeper, CLI"]
    GF -->|"GhosttyKit"| KV
```

| Repo | What it holds |
|---|---|
| kmux | the kmux app and its libraries; upstream Ghostty |
| jemdiggity/ghostty | Ghostty with external I/O (9 files, +598/−38) |
| kanna-v4 | the Kanna app, daemon, keeper and CLI; builds kmux's libraries against the forked GhosttyKit |

kmux only needs to let a build supply its own GhosttyKit. With that, **the kanna-mux fork isn't needed** and can be deleted.

---

## 8. Typing and output

```mermaid
sequenceDiagram
    actor You
    participant G as terminal view (Kanna app)
    participant D as Kanna daemon
    participant P as pty
    participant S as shell
    You->>G: key press
    G->>D: input bytes
    D->>P: write
    P->>S: read
    S->>P: output
    P->>D: read
    D->>D: update the session's screen
    D->>G: output bytes
    G->>You: drawn
```

The keeper isn't in the path of any byte.

---

## 9. Restarting the app

```mermaid
sequenceDiagram
    participant A as Kanna app
    participant D as Kanna daemon
    participant S as shell
    Note over A: quit or crash
    S->>D: output keeps arriving
    Note over A: launch
    A->>A: load its arrangement
    A->>D: attach, subscribe
    D->>A: exact screens + live output
```

---

## 10. Restarting the daemon

```mermaid
sequenceDiagram
    participant A as Kanna app
    participant D as Kanna daemon
    participant K as keeper
    participant S as shell
    Note over D: crash or upgrade
    Note over S: output waits in the kernel buffer
    D->>K: adopt
    K->>D: every live pty (SCM_RIGHTS)
    D->>D: load sessions and screens from disk
    D->>S: SIGWINCH (programs redraw)
    A->>D: reconnect, resubscribe
```

---

## 11. Compared with kanna-v3

| | kanna-v3 | Now |
|---|---|---|
| Processes | keeper + kanna3d + app | keeper + daemon + app: **the same** |
| What the daemon owns | terminals and layout | **sessions**; the app owns its arrangement |
| Daemon's protocol | its own | **kmux's CLI shape** |
| App's panes | terminals only, its own views | **kmux's mux UI**: terminals, web, markdown, iOS |
| Screen state | libghostty-vt from the fork | libghostty-vt from **upstream** |
| App terminals | fork, external I/O | the same fork, rebased |

---

## 12. Build order

```mermaid
flowchart LR
    S1["1. kmux: libraries<br/>CLI shape, mux UI"] --> S2["2. kmux: TerminalBackend seam,<br/>GhosttyKit override"]
    S2 --> S3["3. kanna-v4: daemon + keeper<br/>on kmux's CLI shape"]
    S3 --> S4["4. kanna-v4: Kanna app<br/>on kmux's mux UI"]
```

1. **kmux:** split the code into the libraries (KmuxCore for the CLI shape, KmuxUI for the mux UI) and the app around them, with sessions separate from kmux's arrangement. Reference model and shared cases first.
2. **kmux:** the `TerminalBackend` seam, and a way for a build to supply its own GhosttyKit.
3. **kanna-v4:** the daemon (KmuxCore + libghostty-vt + snapshots) and the keeper. Persistent terminals arrive here.
4. **kanna-v4:** the Kanna app on KmuxUI, with the external-I/O terminal backend; Desk and Shelf owns its arrangement.

Clean-up: kmux's terminal-wrapper hook and `KMUX_RESTORED` are removed; the kanna-mux fork is deleted.

---

## 13. Glossary

| Term | Meaning |
|---|---|
| **CLI shape** | kmux's protocol and sessions model: the commands and what they mean. Kanna's daemon runs it, so the `kanna` CLI matches `kmux`. |
| **Mux UI** | kmux's views: the pane views (terminal, web, markdown, iOS) and the split and tab views. |
| **Session** | One thing a pane shows: a terminal (pty, screen, cwd, exit code), or a web URL, a markdown file, an iOS device and app. |
| **SimulatorKit / CoreSimulator** | Xcode's private frameworks for drawing a simulator device's screen and sending it touches; kmux uses them today. |
| **Arrangement** | Where sessions appear. kmux: windows, tabs, splits. Kanna: Desk and Shelf. |
| **Kanna daemon** | Kanna's background process that owns the sessions. It outlives the Kanna app. kmux has none. |
| **Keeper** | A tiny background process that only creates and holds the ptys, so shells outlive the daemon too. |
| **TerminalBackend** | The seam in the mux UI between a terminal view and where its bytes come from: its own pty (kmux) or Kanna's daemon. |
| **External I/O** | A Ghostty surface mode from the fork: no pty; output is pushed in, keystrokes come out through a callback. |
| **libghostty-vt** | Ghostty's terminal engine as a library, without drawing: keeps a screen, formats it, makes snapshots. |
| **pty** | Pseudo-terminal: what a shell talks to as if it were a real terminal. |
| **SCM_RIGHTS** | Sending open file descriptors, such as pty masters, to another process over a Unix socket. |
| **Snapshot** | A terminal's whole screen state, used to restore it exactly. |
