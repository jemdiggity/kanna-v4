# Kanna's mux: architecture

> 2026-10-09. Proposed. kmux stays an app and also ships libraries. Kanna builds its daemon, keeper and desktop app from two of kmux's layers, the **CLI shape** and the **mux UI**, and its terminals live in its daemon so they survive the app quitting, the way kanna-v3 did.

## Contents

1. [In short](#1-in-short)
2. [Layers and processes](#2-layers-and-processes)
3. [What the daemon keeps and what the app keeps](#3-what-the-daemon-keeps-and-what-the-app-keeps)
4. [How the kanna CLI fits](#4-how-the-kanna-cli-fits)
5. [The repos](#5-the-repos)
6. [Typing and output](#6-typing-and-output)
7. [Restarting the app](#7-restarting-the-app)
8. [Restarting the daemon](#8-restarting-the-daemon)
9. [Compared with kanna-v3](#9-compared-with-kanna-v3)
10. [Build order](#10-build-order)
11. [Glossary](#11-glossary)

---

## 1. In short

| | kmux | Kanna |
|---|---|---|
| What it is | an app, plus libraries | a desktop app, a daemon, a keeper and a CLI |
| Processes | one: the app | three: the app, the daemon, the keeper |
| Terminals live in | the app (Ghostty's own ptys) | the daemon (ptys held by the keeper) |
| After a restart | the layout comes back, terminals are new | the layout comes back, terminals are exactly as they were |
| Ghostty | upstream | the fork (external I/O) |

**Kanna uses two layers of kmux** (see [layers and processes](#2-layers-and-processes)):
- **Layer 3, the mux UI:** the pane views (terminal, web, markdown, iOS). The Kanna app draws with them.
- **Layer 4, the CLI shape:** the protocol (`open`, `list`, `send`, `read`, `close`, …) and terminal sessions. Kanna's daemon runs it, so `kanna` and `kmux` look alike.

**Only terminals live in the daemon.** Everything else a pane shows (a web page's URL and history, a markdown file, an iOS device and app) is layout state, kept and saved by the app with the rest of the layout.

---

## 2. Layers and processes

Read each stack top to bottom, like the OSI model: each layer uses the ones below it. Kanna takes layers 3 and 4 from kmux and adds 1, 2 and 5 of its own.

**Colours:** green is Kanna's code, blue is a kmux library, purple is kmux's own app code, grey is macOS, orange is the CLIs. Outer boxes are processes.

```mermaid
flowchart TB
    subgraph CLIP["CLI processes"]
        CLI["kanna CLI, agents"]
    end
    subgraph APP["Kanna app process"]
        L1["1 · UI<br/>Desk and Shelf<br/>review · editor"]
        L2["2 · Kanna<br/>tasks · agents<br/>worktrees · hooks"]
        L3["3 · Mux UI (kmux)<br/>terminal · web<br/>iOS · markdown views"]
        L1 --> L2 --> L3
    end
    subgraph DAEMON["Kanna daemon process"]
        L4["4 · CLI shape (kmux)<br/>commands · terminals<br/>libghostty-vt screens"]
    end
    subgraph KEEPER["keeper process"]
        L5["5 · Keeper<br/>holds the ptys"]
    end
    subgraph OS["6 · macOS"]
        PTY["ptys<br/>shells, agents"]
        WK["WebKit"]
        SIM["iOS Simulator"]
    end
    CLI -->|"socket"| L4
    L3 <-->|"socket"| L4
    L4 <-->|"socket"| L5
    L5 -->|"creates"| PTY
    L4 <-->|"reads, writes"| PTY
    L3 --> WK
    L3 <--> SIM
    classDef kanna fill:#2f6f4f,stroke:#6fcf97,color:#ffffff
    classDef kmux fill:#2f4f8f,stroke:#7fa7ff,color:#ffffff
    classDef kmuxapp fill:#4f3f7f,stroke:#b39cff,color:#ffffff
    classDef os fill:#4a4a55,stroke:#9a9aa8,color:#ffffff
    classDef ext fill:#6b4f2f,stroke:#e0a960,color:#ffffff
    class L1,L2,L5 kanna
    class L3,L4 kmux
    class PTY,WK,SIM os
    class CLI ext
```

kmux has the same layers without 2 and 5, in one process:

```mermaid
flowchart TB
    subgraph CLIP["CLI processes"]
        CLI["kmux CLI, agents"]
    end
    subgraph APP["kmux app process (the only one)"]
        K1["1 · UI<br/>windows · tabs · splits"]
        K3["3 · Mux UI (kmux)<br/>terminal · web<br/>iOS · markdown views"]
        K4["4 · CLI shape (kmux)<br/>commands"]
        K1 --> K3 --> K4
    end
    subgraph OS["6 · macOS"]
        PTY["ptys<br/>shells, agents"]
        WK["WebKit"]
        SIM["iOS Simulator"]
    end
    CLI -->|"socket"| K4
    K3 <-->|"reads, writes"| PTY
    K3 --> WK
    K3 <--> SIM
    classDef kanna fill:#2f6f4f,stroke:#6fcf97,color:#ffffff
    classDef kmux fill:#2f4f8f,stroke:#7fa7ff,color:#ffffff
    classDef kmuxapp fill:#4f3f7f,stroke:#b39cff,color:#ffffff
    classDef os fill:#4a4a55,stroke:#9a9aa8,color:#ffffff
    classDef ext fill:#6b4f2f,stroke:#e0a960,color:#ffffff
    class K1 kmuxapp
    class K3,K4 kmux
    class PTY,WK,SIM os
    class CLI ext
```

| Layer | Kanna | kmux | Kanna process |
|---|---|---|---|
| 1 · UI | Desk and Shelf, markdown review, code editor | windows, tabs, splits | app |
| 2 · Kanna | tasks, agents, worktrees, hooks | — | app |
| 3 · Mux UI | **kmux library** (KmuxUI): pane views and their layout state | the same | app |
| 4 · CLI shape and terminals | **kmux library** (KmuxCore) + libghostty-vt screens | KmuxCore; terminals are the views' own | daemon |
| 5 · Keeper | creates and holds the ptys | — | keeper |
| 6 · macOS | ptys, WebKit, simulator devices | the same | — |

| Kanna piece | Layers | Built from |
|---|---|---|
| **Kanna app** | 1, 2, 3 | Kanna's UI and Kanna layer + KmuxUI + Kanna's external-I/O terminal backend (forked Ghostty) |
| **Kanna daemon** | 4 | KmuxCore + a libghostty-vt screen per terminal + snapshots on disk |
| **Keeper** | 5 | the `feat/keeper` branch and v3's keeper |
| **`kanna` CLI** | — | KmuxClient (today's `kmux-client`), pointed at Kanna's daemon |

- **The CLI is a client, not a layer:** like the app, it sits on top and calls down into layer 4. Neither serves the other; in Kanna both are clients of the daemon.
- **Why terminals cross into the daemon and nothing else does:** a terminal is a running program, which must not die with the app. A web page, a markdown file or an iOS pane is just state to reopen: a URL, a path, a device. The page itself has to be in the app anyway (WebKit can't draw from another process), and simulator devices already outlive everything.
- **The keeper isn't in the path of any byte:** it creates each pty and keeps a copy, so shells outlive a daemon restart; the daemon reads and writes them itself.

---

## 3. What the daemon keeps and what the app keeps

```mermaid
flowchart LR
    subgraph A["Kanna app: layout state"]
        DESK["Desk and Shelf"]
        P1["pane → t1"]
        P2["pane → t2"]
        P3["web pane<br/>URL, history"]
        P4["markdown pane<br/>file"]
        P5["iOS pane<br/>device, app"]
        DESK --- P1 & P2 & P3 & P4 & P5
    end
    subgraph D["Kanna daemon: terminals"]
        T1["t1<br/>pty, screen, scrollback<br/>cwd, exit code"]
        T2["t2<br/>pty, screen, scrollback<br/>cwd, exit code"]
    end
    P1 -. "by ID" .-> T1
    P2 -. "by ID" .-> T2
    classDef kanna fill:#2f6f4f,stroke:#6fcf97,color:#ffffff
    classDef kmux fill:#2f4f8f,stroke:#7fa7ff,color:#ffffff
    classDef kmuxapp fill:#4f3f7f,stroke:#b39cff,color:#ffffff
    classDef os fill:#4a4a55,stroke:#9a9aa8,color:#ffffff
    classDef ext fill:#6b4f2f,stroke:#e0a960,color:#ffffff
    class DESK,P1,P2,P3,P4,P5 kanna
    class T1,T2 kmux
```

| | What it is | Kept and saved by | After the app restarts |
|---|---|---|---|
| **Terminal** | a pty, its screen and scrollback, cwd, exit code | the daemon | exactly as it was, still running |
| **Layout state** | where panes are, and what each non-terminal pane shows: a web URL and history, a markdown file, an iOS device and app | the app | reopened: the page reloads at its URL (cookies and logins stay on disk), the file reopens, the simulator was never stopped |

In kmux both are in the one app, as today.

---

## 4. How the kanna CLI fits

```mermaid
flowchart TB
    CLI["kanna CLI, agents, hooks"]
    D["Kanna daemon"]
    T["terminals<br/>answered by the daemon"]
    A["Kanna app<br/>layout, web, iOS, markdown"]
    CLI -->|"one socket"| D
    D --> T
    D -->|"passes on"| A
    classDef kanna fill:#2f6f4f,stroke:#6fcf97,color:#ffffff
    classDef kmux fill:#2f4f8f,stroke:#7fa7ff,color:#ffffff
    classDef kmuxapp fill:#4f3f7f,stroke:#b39cff,color:#ffffff
    classDef os fill:#4a4a55,stroke:#9a9aa8,color:#ffffff
    classDef ext fill:#6b4f2f,stroke:#e0a960,color:#ffffff
    class CLI ext
    class D,T kmux
    class A kanna
```

| Command | Touches | Answered by | With the app closed |
|---|---|---|---|
| `list` (terminals), `send`, `read`, `wait`, `close`, `restart` on a terminal | terminals | daemon | works |
| `open term --cmd …` | creates a terminal | daemon | works: shown when the app opens |
| `open … --split right`, `arrange`, `focus`, opening web, markdown or iOS panes | layout | passed to the app | fails clearly, or the CLI starts the app first |
| `navigate`, all `web.*` commands, page text | web panes | passed to the app | the same |
| iOS `tap`, `swipe`, `type`, accessibility tree, screenshots | iOS panes | passed to the app | the same |
| pixel snapshots of any pane | the drawn view | passed to the app | the same |
| later: `kanna task …`, "put this on the desk" | the Kanna layer | the app | needs the app |

The `kmux` CLI is unchanged: it talks to the kmux app, which answers everything itself.

---

## 5. The repos

```mermaid
flowchart LR
    GU["ghostty-org/ghostty<br/>upstream"] -->|"pinned"| KMUX["tampopogk/kmux<br/>app + libraries, MIT"]
    GU -->|"rebased"| GF["jemdiggity/ghostty<br/>kmux-external-io"]
    KMUX -->|"libraries"| KV["jemdiggity/kanna-v4<br/>app, daemon, keeper, CLI"]
    GF -->|"GhosttyKit"| KV
    classDef kanna fill:#2f6f4f,stroke:#6fcf97,color:#ffffff
    classDef kmux fill:#2f4f8f,stroke:#7fa7ff,color:#ffffff
    classDef kmuxapp fill:#4f3f7f,stroke:#b39cff,color:#ffffff
    classDef os fill:#4a4a55,stroke:#9a9aa8,color:#ffffff
    classDef ext fill:#6b4f2f,stroke:#e0a960,color:#ffffff
    class GU,GF os
    class KMUX kmux
    class KV kanna
```

| Repo | What it holds |
|---|---|
| kmux | the kmux app and its libraries; upstream Ghostty |
| jemdiggity/ghostty | Ghostty with external I/O (9 files, +598/−38) |
| kanna-v4 | the Kanna app, daemon, keeper and CLI; builds kmux's libraries against the forked GhosttyKit |

kmux only needs to let a build supply its own GhosttyKit. With that, **the kanna-mux fork isn't needed** and can be deleted.

---

## 6. Typing and output

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
    D->>D: update the terminal's screen
    D->>G: output bytes
    G->>You: drawn
```

The keeper isn't in the path of any byte.

---

## 7. Restarting the app

```mermaid
sequenceDiagram
    participant A as Kanna app
    participant D as Kanna daemon
    participant S as shell
    Note over A: quit or crash
    S->>D: output keeps arriving
    Note over A: launch
    A->>A: load its layout
    A->>D: attach, subscribe
    D->>A: exact screens + live output
```

---

## 8. Restarting the daemon

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
    D->>D: load terminals and screens from disk
    D->>S: SIGWINCH (programs redraw)
    A->>D: reconnect, resubscribe
```

---

## 9. Compared with kanna-v3

| | kanna-v3 | Now |
|---|---|---|
| Processes | keeper + kanna3d + app | keeper + daemon + app: **the same** |
| What the daemon owns | terminals and layout | **terminals only**; the app owns the layout |
| Daemon's protocol | its own | **kmux's CLI shape** |
| App's panes | terminals only, its own views | **kmux's mux UI**: terminals, web, markdown, iOS |
| Screen state | libghostty-vt from the fork | libghostty-vt from **upstream** |
| App terminals | fork, external I/O | the same fork, rebased |

---

## 10. Build order

```mermaid
flowchart LR
    S1["1 · kmux<br/>libraries:<br/>CLI shape, mux UI"] --> S2["2 · kmux<br/>TerminalBackend seam,<br/>GhosttyKit override"]
    S2 --> S3["3 · kanna-v4<br/>daemon + keeper"]
    S3 --> S4["4 · kanna-v4<br/>Kanna app on<br/>kmux's mux UI"]
    classDef kanna fill:#2f6f4f,stroke:#6fcf97,color:#ffffff
    classDef kmux fill:#2f4f8f,stroke:#7fa7ff,color:#ffffff
    classDef kmuxapp fill:#4f3f7f,stroke:#b39cff,color:#ffffff
    classDef os fill:#4a4a55,stroke:#9a9aa8,color:#ffffff
    classDef ext fill:#6b4f2f,stroke:#e0a960,color:#ffffff
    class S1,S2 kmux
    class S3,S4 kanna
```

1. **kmux:** split the code into the libraries (KmuxCore for the CLI shape, KmuxUI for the mux UI) and the app around them, with terminals separate from kmux's layout. Reference model and shared cases first.
2. **kmux:** the `TerminalBackend` seam, and a way for a build to supply its own GhosttyKit.
3. **kanna-v4:** the daemon (KmuxCore + libghostty-vt + snapshots) and the keeper. Persistent terminals arrive here.
4. **kanna-v4:** the Kanna app on KmuxUI, with the external-I/O terminal backend; Desk and Shelf owns the layout.

Clean-up: kmux's terminal-wrapper hook and `KMUX_RESTORED` are removed; the kanna-mux fork is deleted.

---

## 11. Glossary

| Term | Meaning |
|---|---|
| **CLI shape** | kmux's protocol: the commands and what they mean (layer 4). Kanna's daemon runs it, so the `kanna` CLI matches `kmux`. |
| **Mux UI** | kmux's pane views (terminal, web, markdown, iOS) and their layout state (layer 3). |
| **Layout state** | Where panes are and what each non-terminal pane shows (a web URL and history, a markdown file, an iOS device and app). Kept by the app. |
| **SimulatorKit / CoreSimulator** | Xcode's private frameworks for drawing a simulator device's screen and sending it touches; kmux uses them today. |
| **Layers** | The stack in [section 2](#2-layers-and-processes), read top to bottom like the OSI model. |
| **Kanna daemon** | Kanna's background process that owns the terminals. It outlives the Kanna app. kmux has none. |
| **Keeper** | A tiny background process that only creates and holds the ptys, so shells outlive the daemon too. |
| **TerminalBackend** | The seam in the mux UI between a terminal view and where its bytes come from: its own pty (kmux) or Kanna's daemon. |
| **External I/O** | A Ghostty surface mode from the fork: no pty; output is pushed in, keystrokes come out through a callback. |
| **libghostty-vt** | Ghostty's terminal engine as a library, without drawing: keeps a screen, formats it, makes snapshots. |
| **pty** | Pseudo-terminal: what a shell talks to as if it were a real terminal. |
| **SCM_RIGHTS** | Sending open file descriptors, such as pty masters, to another process over a Unix socket. |
| **Snapshot** | A terminal's whole screen state, used to restore it exactly. |
