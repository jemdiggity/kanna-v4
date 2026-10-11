---
name: kanna-tasks
description: Use when running in a Kanna terminal ($KANNA_PANE is set), as soon as you know what the work in this session is, and whenever the user plans work with you or you split off work for later.
---

# Kanna tasks

In Kanna, each agent's work is a **task**: one agent in one repo, shown on a Desk. A task you start in is "New task" until you name it, and its name is what the user sees in the title bar, on the Shelf and in the backlog.

## Name your task

Once you know what the work is (from the first message, or the user's first reply), name it:

```sh
kanna task name "Fix login redirect"
```

- **Short:** two to five words, what the work is, like a branch name in plain English.
- **Once,** then again only if the work turns into something else.
- **Not for a task that was started with a name:** if it has one (`kanna task list` shows it), keep it.

## Name your branch

A task works in a worktree of its own (`.kanna-worktrees/3f9a1c2e`) on a branch with a random name (`kanna/3f9a1c2e`). Once you know the work, rename the branch, from the worktree:

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
- **Don't start tasks yourself.** The user moves them to the Shelf; start one (`kanna task start`) only when they ask.

## Commands

| Command | Does |
|---|---|
| `kanna task name [TASK] NAME` | Name a task (default: the one you're in) |
| `kanna task new NAME [FIRST-MESSAGE] [--agent CMD]` | Add a task to this repo's backlog |
| `kanna task list` | Every task: ID, where (a Desk, the Shelf, the backlog), status, repo, name |
| `kanna task start TASK [--window WINDOW]` | Move a backlog task to the Shelf: worktree, branch, agent, first message |
| `kanna task done [TASK]` | End a task, or take it off the backlog |

Run them from your own terminal, in the repo: a new task belongs to the repo you're in.
