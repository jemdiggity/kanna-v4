# New Desk

**Status:** opening a Desk is built; its planning terminal starting the planning agent is planned ([2](#2-its-planning-terminal)) · **Mockup:** [`references/mockups/desk.html`](../../references/mockups/desk.html) (Simulate: no tasks, first launch, Installed, Forget the project's planner; ⌘N) · **Part of:** [Desk](desk.md), [planning terminal](planning-agent.md) · **Builds on:** [agent setup](agent-setup.md), [settings](settings.md), [tasks](tasks.md#which-agents-resume)

What happens when a Desk window opens with nothing in it: where it opens, and how its planning terminal gets you to a planning agent.

![A new Desk: its planning terminal asks which agent plans and makes tasks](task-start.png)

![Then it offers to make that agent the project's](planner-save.png)

![The planning agent starts and greets you beside the Shelf](desk-empty.png)

![Quit it and you're in your shell, with how to start it again](planner-quit.png)

![File → New Desk: in ~, not a project, the choice isn't offered as the project's, and the agent asks which project](desk-new.png)

**Done when** a new Desk on a repo asks, starts the agent you pick, and that agent uses the kanna-planning skill, with Claude and with Codex; and a new Desk on a project whose planner is set starts it without asking.

## Contents

1. [When one opens](#1-when-one-opens)
2. [Its planning terminal](#2-its-planning-terminal)
3. [How it's built](#3-how-its-built)
4. [Tests](#4-tests)
5. [Rejected](#5-rejected)

## 1. When one opens

| How | Its folder |
|---|---|
| Kanna opens with nothing | the shell's folder Kanna was started from (`kanna …`, passed as `KANNA_START_DIR`), else `~` |
| File → New Desk (⌘N) | always `~`, as `kanna ~` would; `cd` it into the project |
| `kanna PATH` | PATH; the window keeps that project across a relaunch, until a task from another project is pulled onto it |

- **It shows the Shelf:** the planning terminal on the left, the full Shelf of the folder's repo on the right ([planning terminal](planning-agent.md#1-when-it-shows)). The title bar says Shelf; there's no folder picker.
- **⌘N always means New Desk.** New Window (a plain terminal window) keeps its menu item, without a shortcut. There's no New Instance: another project is what a Desk is for.

## 2. Its planning terminal

```mermaid
flowchart LR
    S["agent setup<br/>(asks once per launch)"] --> P{"planning.agent set,<br/>or one agent installed?"}
    P -->|"yes"| A["the agent, first message:<br/>You are the Kanna planning agent."]
    P -->|"no"| Q["which agent plans?"] --> O["make it the project's?<br/>(in a git repo)"] --> A
    A -->|"you quit it"| SH["your shell"]
```

- **It runs Kanna's planner script once:** when the Desk opens, or when its terminal is restarted (`kanna restart`). Each run starts a new conversation.
- **Agent setup first:** `kanna agents setup --ask` and `update --ask` ([agent setup](agent-setup.md#3-asking)).
- **The choices** are the agents Kanna knows that your shell finds, in the `AGENTS` table's order; Return takes the first.
- **No question** when the folder's `planning.agent` is set ([settings](settings.md)) or only one agent is installed. With none, it says so and you're in your shell.
- **Make it the project's:** yes writes `planning.agent` to the repo's `.kanna/config.json`, part of the repo, so every new Desk on the project, yours or anyone's, starts that agent. Not offered outside a git repo (`~`). No saves nothing, and the next Desk asks again.
- **The agent starts as you'd start it,** in the terminal's folder, with `You are the Kanna planning agent.` as its first message, so it answers at once with what's running and waiting (in `~`, which project). It asks permission as it normally does. Codex runs with `--no-daemon` ([tasks](tasks.md#7-how-its-built)).
- **Quit it and you're in your shell,** with a line giving the command that starts it again as the planner. Anything you start there is yours.

## 3. How it's built

- **The app asks for a planning terminal** (`backlog` `plan`) with the window's folder; `task desk` makes the window, with a folder or none (started from Finder).
- **`planner.sh`, in `Kanna.app/Contents/Resources`,** copied there by `scripts/build-kanna-app.sh`. It replaces `getting-started.md`.
- **The daemon passes it everything** (`planner_command` in `backlog.rs`): the `kanna` beside the daemon, the folder's `planning.agent` if set, and each agent Kanna knows as a name and the command that starts it with the planner's first message, built from the `AGENTS` table as a task's agent is (its first-message flag, `--no-daemon` for Codex; no session, no skipping permission prompts). The script knows no agent itself.
- **The terminal's command** is the [settings](settings.md) exports, then `planner.sh …`, then your login shell with Kanna's zsh integration put back. It runs in your interactive login shell, so `command -v` finds what your `.zshrc` sets up (nvm's codex).
- **Writing the setting** uses `plutil` on the repo's `.kanna/config.json` (making it with `"version": 1` if it's missing), keeping its other keys.
- **No marker variable:** the planning terminal has `KANNA_PANE` and `KANNA_SOCKET` like every Kanna terminal, and no `KANNA_PLANNER`; the daemon knows which pane is a window's planner.

## 4. Tests

- **Unit (`backlog.rs`):** the planning terminal's command for each agent; `planner.sh` run with stand-in agents: asks with two, not with one or with `planning.agent` set, writes the setting on yes and keeps the file's other keys, says so with none.
- **e2e (`scripts/e2e-kanna.sh`):** a new Desk asks, yes makes the agent the project's, and the next new Desk starts it straight away; claude and codex against the mock model (`tools/agent-harness`) each receive `You are the Kanna planning agent.` as their first message.

## 5. Rejected

- **A `kanna plan` command:** it only makes sense inside a planning terminal, not as a general command.
- **Resuming the planner's conversation:** it'd need a session kept per window; the planner reads the Shelf and backlog when it starts, so a new conversation loses little.
- **Telling it in launch instructions** (`claude --append-system-prompt`, `codex -c developer_instructions`): started without a message, an agent waits silently, and each agent needs its own flag.
- **Remembering your answer without asking:** you choose per Desk, and save it for the project only when you say so.
- **Printing getting-started text** (before): the agent itself says what's running and waiting.
- **Guessing a folder** (the latest parked or backlog task's repo) when Kanna starts with none: there's no project concept yet; the folder is where Kanna was started, else your home folder, and you `cd`.
- **A folder picker for New Desk:** you `cd` the planning terminal, and the Shelf follows.
