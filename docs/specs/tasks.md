# Tasks

**Status:** built · **Mockup:** [`references/mockups/desk.html`](../../references/mockups/desk.html) (Simulate: no tasks; park to see the backlog; `kanna task new` in an agent's pane) · **Builds on:** [Desk](desk.md) · **From:** Kanna v2's tasks (a worktree per task) and v3's (a task belongs to a repo, and waits until it's pulled)

A **task** is one agent's work in one repo. Agents make tasks: they add them to the repo's **backlog**, and you move the ones you want to the Shelf, where each gets its own agent, branch and worktree. There's no form: a repo with no tasks starts you on a terminal where you run your agent and tell it what you want.

![A repo with no tasks: a terminal for your agent, and how to start](task-start.png)

![An empty Desk: the Shelf's tasks above, the backlog below](task-backlog.png)

## Contents

1. [What a task is](#1-what-a-task-is)
2. [How tasks are made](#2-how-tasks-are-made)
3. [The backlog](#3-the-backlog)
4. [Naming](#4-naming)
5. [Done](#5-done)
6. [CLI](#6-cli)
7. [How it's built](#7-how-its-built)
8. [Open questions](#8-open-questions)
9. [Glossary](#9-glossary)

## 1. What a task is

```mermaid
stateDiagram-v2
    direction LR
    [*] --> backlog: an agent runs kanna task new
    [*] --> shelf: Start a task
    backlog --> shelf: Move to Shelf (agent, branch, worktree)
    shelf --> desk: pull
    desk --> shelf: park
    backlog --> [*]: Done
    shelf --> [*]: Done
    desk --> [*]: Done
```

- **One repo.** A task belongs to the repo it was made in, so it can later be sent to another machine or person with that repo.
- **A name and a first message.** In the backlog, that's all a task is.
- **On the Shelf, it runs:** its own worktree `<repo>/.kanna-worktrees/<NAME>` on branch `<NAME>`, made from the repo's checked-out commit, and an agent there that gets the first message. NAME is `task-` and the first 32 bits of a fresh random UUID in hex (`task-3f9a1c2e`), so it never collides with another worktree or branch; the agent renames the branch once it knows the work ([4](#4-naming)).
- **Every task works in a worktree.** All development happens in one, never in the repo's own checkout.
- **Kanna is git-based: a task needs a git repo.** In a folder that isn't one, making a task is refused; Kanna never runs `git init` itself. An agent sets the repo up with you (in a new, empty project, the planning agent does), and a repo with no commits yet works: its first task's branch starts empty (`git worktree add --orphan`), so the task's first commit is the project's. Kanna never commits for you.
- **At most three run per repo**: the one on the Desk and two on the Shelf. The backlog has no limit.

## 2. How tasks are made

- **By an agent:** `kanna task new "NAME" "FIRST MESSAGE"` adds a task to the backlog of the repo it runs in. An agent planning with you, or one splitting its own work, makes tasks this way.
- **Start a task** (⌘N on a Desk, where it takes over from New Window; or the button on an empty Desk): a new task on this Desk with an agent in its own worktree and no first message. You tell the agent what to do; it names the task.
- **A repo with no tasks** starts on a task with a plain terminal and a Getting started tab (pictured above): when Kanna opens with nothing, and when a Desk empties with no tasks running or waiting. You `cd` to your repo and run your agent there; the Desk follows the terminal's folder.
- **`kanna PATH` opens a Desk on a project:** a new window on the project PATH is in, with nothing docked. It shows the project's Shelf and backlog, or the starter terminal (in PATH) when the project has no tasks, and takes none of its three running slots. The window keeps the project across a relaunch, until a task from another project is pulled onto it.

## 3. The backlog

- **The empty Desk lists it** under the Shelf: each task's name, its first message, and the task that made it.
- **Move to Shelf** starts a task: it makes the worktree, starts the agent with the first message, and puts it on the Shelf. When three already run, it's refused until one is Done.
- **Oldest first.** A Desk's Shelf strip shows how many wait.

## 4. Naming

- **The agent names its task** with `kanna task name "NAME"` once it knows what the work is. Until then it shows its agent's terminal title as is, status glyph and all (`◑ Claude Code`), or "New task" when there isn't one.
- **It learns how from Kanna's `kanna-tasks` skill** ([`.agents/kanna-skills/skills/kanna-tasks`](../../.agents/kanna-skills/skills/kanna-tasks/SKILL.md)), which also tells it to write backlog first messages that stand alone and to leave starting tasks to you.
- **A backlog task's name** is the one its maker gave it, kept when it starts.
- **The agent renames its branch** too, with `git branch -m NEW` in its worktree, from `task-3f9a1c2e` to a short name for the work. The worktree's folder keeps its name: the agent and its terminals are in it.

## 5. Done, or back to the backlog

- **Done ends the task:** its agent and tabs close, or it leaves the backlog.
- **The worktree and branch stay.** Cleaning them up is yours for now.
- **Move to Backlog puts a running task back** (Task menu, or `kanna task defer`), freeing its slot of the three: its agent and tabs close and it waits in the backlog again, under its old ID if Kanna started it, with its worktree. Starting it again opens the agent in that same worktree, and a new one only if the worktree is gone.
- **A claude agent comes back to its conversation.** Kanna starts claude with a session ID of its own (`claude --session-id UUID`), keeps it with the running task, and resumes it (`claude --resume UUID`). A claude task Kanna didn't start resumes the folder's latest conversation (`claude --continue`). Other agents start again with the first message, their conversation lost, until they resume too ([open questions](#8-open-questions)).
- **Deferring stops the agent.** Whatever it was in the middle of is interrupted; its files and conversation are kept.

## 6. CLI

| Command | Does |
|---|---|
| `kanna task new NAME [FIRST-MESSAGE] [--agent CMD]` | Add a task to this repo's backlog (the agent defaults to `$KANNA_AGENT`, else `claude`, and starts without asking permission) |
| `kanna task start TASK [--window WINDOW]` | Move a backlog task to the Shelf (or onto that Desk): worktree, agent, first message |
| `kanna task name [TASK] NAME` | Name a task (default: the one you're in) |
| `kanna task list` | Every task: ID, where (a Desk, the Shelf, the backlog), status, repo, name |
| `kanna task defer [TASK]` | Move a running task back to the backlog, keeping its worktree and its claude session (default: the one you're in) |
| `kanna task done [TASK]` | End a task, or take it off the backlog |

`park` and `pull` are the [Desk's](desk.md#4-cli).

## 7. How it's built

```mermaid
flowchart TB
    CLI["kanna task new / start / name"] --> D
    APP["Kanna.app: empty Desk"] --> D
    D["daemon: the backlog<br/>ID · name · repo · first message · maker"] -->|"start: git worktree add, open --dock on the Shelf"| L["layout: Desk / Shelf"]
```

- **The daemon keeps the backlog**, saved with the terminals, so agents add tasks with or without the app.
- **A running task is its docked agent pane**, as on the [Desk](desk.md): its name is the pane's name, its repo the pane's folder.
- **Only a task's agent docks.** `kanna move PANE --dock` moves the docked pane to the other edge or another window, and refuses any other pane (`not_a_task`), so a task starts only by starting one and the limit holds.
- **Starting** a backlog task checks the limit, runs `git worktree add`, opens the agent docked straight onto the Shelf in that folder with the first message, names the pane, and drops the backlog entry.
- **An ID is never handed out twice.** A new task's ID is saved as it's made, whether it waits in the backlog or starts at once (Start a task), so a restart can't reuse one. IDs name backlog entries only; worktrees and branches get their own random names, so a lost `backlog.json` or another Kanna on the same repo can't collide with them.
- **A task's agent doesn't ask permission.** Starting one adds the agent's own flag for that (`claude --dangerously-skip-permissions`, `codex --yolo`) unless its command already says how to ask (`--permission-mode`, `--ask-for-approval`/`-a`). The task works in its own worktree, and you watch it on the Desk. The backlog keeps the command as written.

## 8. Open questions

1. **Task IDs that travel:** backlog IDs (`t4`) are this machine's; sending tasks elsewhere will want IDs that don't clash.
2. **Resuming every agent:** only claude comes back to its conversation after Move to Backlog; codex and the others need their own session IDs and resume (backlog task t15).

## 9. Glossary

| Term | Meaning |
|---|---|
| **Task** | One agent's work in one repo, shown on a Desk |
| **Backlog** | A repo's tasks that aren't running yet: a name and a first message each |
| **Repo** | The git repository a task belongs to |
| **Worktree** | A second checkout of the repo, in `.kanna-worktrees/`, on the task's own branch |
| **First message** | What the agent is told when its task starts |
