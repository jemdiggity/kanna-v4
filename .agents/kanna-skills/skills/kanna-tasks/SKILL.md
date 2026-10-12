---
name: kanna-tasks
description: Use when running in a Kanna terminal ($KANNA_PANE is set), as soon as you know what the work in this session is, and whenever the user plans work with you or you split off work for later.
---

# Kanna tasks

In Kanna, each agent's work is a **task**: one agent in one repo, shown on a Desk. A task you start in is "New task" until you name it, and its name is what the user sees in the title bar, on the Shelf and in the backlog.

## One project, one Desk

A project (a repo) has **one Desk**: its window. The Desk shows one task at a time, its agent docked on the left and its tabs on the right; the project's other tasks wait on the **Shelf** along the bottom of the same window, still running. "The Desk" always means that window.

- **New tasks go in the backlog** (`kanna task new`). Running and place are separate: `kanna task start` runs a task where it is (a backlog task gets its agent and stays in the backlog, and any number can run there), `kanna task stop` ends its agent and leaves it where it is, and `kanna task shelve` moves a backlog task to the Shelf, starting it if it's stopped. Shelve one when the Shelf has room (three tasks per project, counting the Desk's) or when the user asks. When the user asks and the Shelf is full, make room by moving a Shelf task back to the backlog (`kanna task defer TASK`; it stays as it is, running or stopped). The user brings a Shelf task onto the Desk when they want it.
- `kanna task pull TASK` swaps a task onto the Desk, parking the one there; `kanna task park` moves the Desk's task to the Shelf. Only when the user asks.
- Never make another Desk for a project (`--window new`): there's one.

If `$KANNA_PLANNER` is set, you're a window's **planning agent**, not a task: use the `kanna-planning` skill instead, and skip naming your task and branch.

## Name your task

Once you know what the work is (from the first message, or the user's first reply), name it:

```sh
kanna task name "Fix login redirect"
```

- **Short:** two to five words, what the work is, like a branch name in plain English.
- **Once,** then again only if the work turns into something else.
- **Not for a task that was started with a name:** if it has one (`kanna task list` shows it), keep it.

## Name your branch

A task works in a worktree of its own (`.kanna-worktrees/task-3f9a1c2e`) on a branch with a random name (`task-3f9a1c2e`). In a new project the branch has no commits yet: your first commit is the project's first. Once you know the work, rename the branch, from the worktree:

```sh
git branch -m fix-login-redirect
```

- **Short kebab-case,** what the work is; follow the repo's branch prefixes if it has any.
- **Only the branch:** leave the worktree's folder where it is, since you and your terminals are in it.
- **Before you push,** so the remote branch has the real name.

## Add tasks to the backlog

When you and the user plan work, or you find work that belongs in its own task, add it to this repo's backlog rather than doing it now:

```sh
kanna task new "Add rate limiting" "Add rate limiting to the login endpoint: ..."
```

- **The first message stands alone.** The agent that gets it starts fresh in its own worktree and hasn't seen this conversation: say what to do, why, where (files, functions), and how to know it's done.
- **Start or shelve one only when the user asks, or shelve when there's room.** `kanna task shelve` puts it on the Shelf if the project has fewer than three tasks out (the Desk's and the Shelf's); otherwise leave it in the backlog, unless the user asks, and then make room by deferring a Shelf task they're not using. `kanna task start` runs it in the backlog instead, with no limit.

## Commands

| Command | Does |
|---|---|
| `kanna task name [TASK] NAME` | Name a task (default: the one you're in) |
| `kanna task new NAME [FIRST-MESSAGE] [--agent CMD]` | Add a task to this repo's backlog |
| `kanna task list [QUERY]` | Every task: ID, name, where (a Desk, the Shelf, the backlog), state (working, idle, needs you, or stopped), repo; QUERY keeps those whose name or first message has every word |
| `kanna task start TASK` | Run a task where it is: a backlog task gets its worktree, branch and agent with the first message, and stays in the backlog; a stopped task resumes its conversation |
| `kanna task stop [TASK]` | End a task's agent and tabs, leaving it where it is (only when the user asks) |
| `kanna task shelve TASK` | Move a backlog task to the Shelf, starting it if it's stopped (three per project) |
| `kanna task pull TASK` | Bring a task onto the Desk, parking the one there (only when the user asks) |
| `kanna task park [TASK]` | Move the Desk's task to the Shelf (only when the user asks) |
| `kanna task defer [TASK]` | Move a task back to the backlog as it is, running or stopped (only when the user asks) |
| `kanna task done [TASK]` | End a task, or take it off the backlog |

Run them from your own terminal, in the repo: a new task belongs to the repo you're in.
