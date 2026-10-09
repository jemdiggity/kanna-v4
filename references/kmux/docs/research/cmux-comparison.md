# cmux vs kmux — Feature Comparison

> Research for the kmux roadmap, 2026-10-09. From cmux's public repo and docs (sources at the end) and kmux's spec and CLI help.

### 1. What cmux is

cmux is a free, open-source macOS terminal app from Manaflow (a YC company). It is written in Swift and AppKit and embeds libghostty; it is not a fork of Ghostty. It is built for developers who run many AI coding agents at once (Claude Code, Codex, OpenCode, Gemini CLI, Amp). Its main additions over a plain terminal are a vertical sidebar of workspaces with git, PR, port and notification metadata, "notification rings" that show which agent needs you, a scriptable browser pane, and a CLI and socket API that can do everything the UI does. It is very active: about 28k stars, latest stable **v0.65.0 on 2026-10-05**, last push 2026-10-08, and frequent nightly and FFI prereleases. The repo also holds **cmux-tui**, a Rust tmux-style multiplexer with a headless server, which runs on macOS and Linux.

### 2. Comparison

| Feature | cmux | kmux | Notes |
|---|---|---|---|
| Terminal pane | Ghostty (libghostty), GPU-rendered | Ghostty (GhosttyKit, upstream) | Same engine |
| Web pane | Yes, scriptable: accessibility/DOM snapshot, click, fill, eval JS, console and network | Yes: URL, ⌘L, optional back/forward, waits for localhost | cmux's is much richer for agents |
| iOS Simulator pane | Not found | Yes, embedded framebuffer with touch input | cmux has an iOS *companion app* (TestFlight), which is not a simulator pane |
| Layout | Windows, workspaces (vertical sidebar), splits | Windows, tabs, split tree with fractional sizes, `arrange` with a layout tree, zoom, drag-to-dock | Whether cmux has fractional sizing or declarative arrange is unconfirmed |
| Control transport | Unix socket (`/tmp/cmux.sock`), newline-delimited JSON-RPC `{id,method,params}`; access modes off, cmux processes only (default), allowAll | Unix socket per instance, newline-delimited JSON `{id,cmd,args}`, user-only permissions | Very similar |
| CLI | `cmux` covering workspaces, splits, send text and keys, notify, sidebar status/progress/log, browser, identify | `kmux` in Rust: 17 commands, `commands --json`, "did you mean" suggestions, typed exit codes | kmux's CLI is more deliberately discoverable; cmux's covers more ground |
| Reading pane contents | Yes: read screen, screenshots | **No** | |
| Events / subscribe | Yes in cmux-tui (`subscribe`); unconfirmed for the macOS app API | None, clients poll `list` | |
| Agent attention | OSC 9/99/777, `cmux notify`, blue ring, unread badges, desktop notifications, ⌘⇧U jumps to latest unread | None | |
| Sidebar metadata | Git branch, PR, cwd, ports, status, progress | None (panes are chromeless) | |
| Agent hooks and skills | `cmux hooks setup` for Claude Code, Codex, OpenCode; a skills library | None | |
| Config and shortcuts | Reads `~/.config/ghostty/config` (incl. keybinds); own `cmux.json`; shortcuts editable in Settings | Shortcuts come from the Ghostty config, with defaults | |
| Persistence | Restores windows, workspaces, panes, cwd, best-effort scrollback; agent resume by session ID; `local-tmux` for live detach | None | |
| Multiple instances | Not found | Yes, `--instance`, one socket each | |
| Background launch for tests | Not found | `--bg` / `KMUX_BG` | |
| Remote / SSH | `cmux ssh` with a Go `cmuxd-remote` daemon; browser traffic goes through the remote; remote tmux attach (beta) | None | |
| Architecture | macOS app runs its own panes; cmux-tui has a durable headless server with attaching clients | Single app; a daemon is planned | See section 5 |
| Platforms | macOS app; cmux-tui on macOS and Linux; iOS beta | macOS only | |
| Performance | Qualitative only ("fast startup, low memory", GPU) | Measured with `kmux-bench`: first output p95 52 ms, typing p95 0.6 ms, 9 MB per pane | No cmux numbers found |
| Distribution | DMG with Sparkle auto-update, Homebrew cask, nightlies, `npx cmux` for the TUI | Built from source | Whether cmux is notarized is unconfirmed (the README warns about a first-launch prompt) |
| License | GPL-3.0-or-later; server parts (web, workers, relays) BUSL-1.1 | Not stated in the files I read | |

### 3. cmux has, kmux lacks (ranked)

1. **Reading pane contents and screenshots.** An agent needs this to check its own work. kmux can only send text into a pane.
2. **Attention signals** (OSC 9/99/777 and a notify command, shown on the pane). This is the core reason cmux exists: knowing which agent is waiting on you.
3. **Browser automation** (DOM snapshot, click, fill, eval, console). It would make kmux's web pane something an agent can test with, not just look at.
4. **Event subscription.** Polling `list` doesn't scale, and the spec already lists events as possible later.
5. **Session restore and agent resume.** Users expect layouts to survive a restart.
6. **Status and progress metadata per pane or tab.** It can be shown without adding chrome (tab title or badge, say).
7. **Hook installers and skills for common agents.** These make the first run with a given agent quick to set up.
8. **SSH / remote workspaces.** This only matters if kmux goes client/server.
9. **Packaging and auto-update** (Sparkle, Homebrew).

### 4. kmux has, cmux lacks

- An embedded iOS Simulator pane with touch input.
- A declarative `arrange` layout tree with fractional sizes, snapping and drag-to-dock or swap (cmux's equivalent is unconfirmed).
- Isolated multiple instances, each with its own socket, plus a background launch mode for tests.
- A published, measured performance benchmark with pass/fail targets.
- An agent-discoverable CLI (`commands --json`, typed exit codes, a "not in the running kmux" marker), with protocol cases shared between the reference model and the app.

### 5. Roadmap questions

- **Linux:** the cmux macOS GUI app does **not** run on Linux. **cmux-tui** (Rust, built on libghostty-vt) supports macOS and Linux, with Windows via ConPTY "planned for phase 2". A Linux `cmux-browser-host` build was released 2026-10-07. So cmux's Linux story is a TUI, not a GUI.
- **Client/server:** yes, in two places.
  1. **cmux-tui:** `server start --session X` runs a durable headless server. TUIs, GUIs and agents attach over a Unix socket using JSON Lines (raw protocol v12, plus a public resource API `cmux.protocol/2`). It can also be reached through `cmux relay` over `ssh -T`, WebSocket, Iroh, or HTTP on loopback with a bearer token. Network clients authenticate with per-device keys inside Noise sessions.
  2. **cmuxd-remote (Go):** it is installed over SSH and serves newline-delimited JSON over stdio, using `session.*`, `pty.*` and `proxy.*` methods. Remote PTYs survive the local app closing or relaunching.

  The macOS app itself appears to host its terminals in-process, as kmux does today. cmux recommends `cmux local-tmux` for live detach on the local machine, which suggests the GUI does not yet use a local daemon. That last point is inferred, not confirmed. A `CmuxTerminalClient.xcframework` for iOS and macOS (2026-09-30) hints that the GUI and the iOS app are moving onto that same client/server protocol.

### 6. Sources

- https://github.com/manaflow-ai/cmux (README, LICENSE)
- https://api.github.com/repos/manaflow-ai/cmux/releases
- https://github.com/manaflow-ai/cmux/blob/main/daemon/remote/README.md
- https://github.com/manaflow-ai/cmux/blob/main/cmux-tui/README.md, cmux-tui/docs/remote.md, cmux-tui/docs/getting-started.md, cmux-tui/docs/protocol.md
- https://cmux.com and https://cmux.com/docs/api
- https://www.ycombinator.com/launches/PbB-cmux-the-open-source-terminal-built-for-coding-agents
- For kmux: ~/work/kmux/docs/kmux-spec.md (v0.7), ~/work/kmux/README.md, and the output of `kmux help` and `kmux commands --json`

**Not confirmed:**
- Whether cmux has an iOS simulator pane, multiple instances, notarization, or event subscription in the macOS app's API.
- cmux performance figures: none were published that I could find.
- kmux's license.
