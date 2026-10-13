---
name: kanna-quiz
description: Use in Kanna when $KANNA_QUIZ is set (you're the agent of a Quiz window), or as a planning agent offering the user a quiz on what landed on main: five multiple-choice questions about the codebase, asked one at a time, each answer explained with the code opened at the line.
---

# Kanna quiz

Agents write code faster than the user can read it. A quiz is a short break that keeps them up with what landed: five multiple-choice questions about main's last day of commits, asked one at a time, each answer explained, with the code shown beside you on a miss. About two minutes. No score to protect.

Two parts: the planning agent **offers** a round and opens the Quiz window; the agent there (`$KANNA_QUIZ` is set) **asks** it.

## Offering (the planning agent)

When your session starts (once per session):

1. `git log --oneline main --since=1.day` (`master` if there's no `main`). With nothing, say nothing about a quiz.
2. Otherwise offer once, in a line, after whatever else you say: what landed (the work, not the hashes), then "Quiz on today's changes? Five questions, about two minutes."
3. On yes: if `kanna list` shows a pane named `quiz`, `kanna focus quiz`. Else open the Quiz window, with the agent you are:

   ```sh
   kanna open --window new --name quiz --cwd "$(git rev-parse --show-toplevel)" \
     --cmd 'KANNA_QUIZ=1 claude "Quiz me on what landed on main."'
   ```

   For Codex, the command is `KANNA_QUIZ=1 codex --no-daemon "Quiz me on what landed on main."`. Say it's open, and carry on.

## Asking (the quiz agent)

You're in the repo's own folder, on main, with no task: a quiz changes nothing. Don't name a task, rename the branch, commit or edit files.

### Read first

- `git log --stat main --since=1.day` (at most 30 commits). With none, say so and offer the last 30 commits instead.
- The commit messages, the diff (`git show`, `git diff`), and the specs and docs those commits touch. Only what's true on main: never an agent's conversation, never another branch.
- If the checkout isn't on main (`git rev-parse --abbrev-ref HEAD`), read with `git show main:PATH`, and tell the user the code you open is the checkout's, not main's.
- Pick five topics, spread across files, broadest first: how the pieces fit before a line's detail. Say what landed in a line and that the first question is coming; don't make the user wait for all five.

### The kinds

| Kind | Asks | Example |
|---|---|---|
| Architecture | how the pieces fit: who owns what, what talks to what | Which process owns a pane's terminal: the app, the daemon, or the CLI? |
| Pick the drawing | the same, as three diagrams, two wrong | Which drawing shows how a terminal's bytes reach Kanna.app? |
| Behaviour | what happens when | When you Move to Backlog, what happens to the agent? |
| Where it lives | which file or function does it | Which file decides a pane's status? |
| Why | a decision, from the specs | Why does an instance have its own daemon? |
| Spot the bug | a few lines of today's code with one changed; which line? | Which of these lines has a bug put in? |

Mix them; a round has at least one architecture or drawing question, first.

### One question at a time

Write each question just before you ask it:

- **Four choices** (three for a drawing), one right, the wrong ones plausible: things a reader who skimmed might believe. Not trick questions.
- **About some lines:** read the file and pick the lines that answer it (a function, a spec's diagram, the place one piece hands off to another), so you can open them on a miss with the right line numbers.
- **Ask it with your multiple-choice tool** (Claude's AskUserQuestion: the question, choices as options with a short description; header `Quiz N/5`). An agent without one: a numbered list, and the user answers with a number.
- Head it, in the chat: `**N of 5** · KIND · where it comes from` (a commit's short hash, or a spec's path).
- **Spot the bug:** put the lines in the question itself, labelled A to D with their line numbers, with exactly one changed to put a bug in; the choices are A to D. Not only in the chat before it: the question tool can cover what you wrote.
- **Pick the drawing:** write three mermaid diagrams under the headings `## A`, `## B`, `## C`, with the question above them, to `$TMPDIR/kanna-quiz/drawings.md`, and open it beside you: `kanna open md "$TMPDIR/kanna-quiz/drawings.md"`. The choices are A, B and C, each described only as "Drawing A" and so on: describing a drawing in words gives it away. Close it (`kanna close PANE`) when you move on.
- **Never hint,** never reword a choice once asked, and never say the answer before the user picks.

### Answering

- Say **Right.** or **Not quite:** it's N, and the answer; then why, in a sentence or two. Every answer gets the why.
- **On a miss,** open the code straight away, with the why on its lines:

  ```sh
  kanna comment crates/kanna-daemon/src/agents.rs:514 "The daemon keeps each file as it installed it and runs git merge-file on yours, the base and the new one."
  ```

  It splits beside you (`FILE:LINE` takes the whole scope at that line; `FILE:FIRST-LAST` takes just those lines). A comment on the same file reuses that pane; before one on another file, close the last code pane you opened (`kanna close PANE`; `kanna comment` printed its ID).
- **On a right answer,** offer to open the code; open it if asked.
- **Follow questions** the user asks between questions: answer from the code, briefly (comment on it if that's clearer), then go on to the next question.

### At the end

- Say what they knew: "You knew 4 of 5 about today's changes." Then one line per question: ✓ or ✗, its topic, and its file and line.
- Ask: close this window, or another round? Another round picks new topics from the same commits, or older ones once those run out.
- To close: close the panes you opened, then your own window (`kanna close --window WINDOW`, the one `kanna list` shows your pane in).

Nothing is kept between rounds: no files, no history.

## Commands

Run the `kanna` on your PATH, from this terminal: its `KANNA_*` variables tell Kanna which pane is asking, so what you open goes beside you.

| Command | Does |
|---|---|
| `kanna comment FILE:LINE[-LAST] TEXT` | Show the code beside you with TEXT on those lines |
| `kanna open md FILE` | Show a markdown file (with mermaid diagrams) beside you |
| `kanna close PANE` | Close a pane you opened |
| `kanna focus PANE` | Bring a pane forward |
| `kanna list` | Windows, tabs and panes, with their IDs |
