# Doc Markup

**Status:** proposed · **Mockup:** [`docs/mockups/doc-markup.html`](../mockups/doc-markup.html) · **From:** kanna v3 slice 6b, "spec markup"

**Doc markup** lets you comment on a markdown file in its pane, and have the agent working beside it read, answer and resolve those comments. You review a spec with an agent the way you would with a colleague: point at a passage, say what's wrong, see it fixed.

![A markdown pane with two comment threads, beside the agent's terminal they were sent to](doc-markup.png)

## Contents

1. [The loop](#1-the-loop)
2. [Rules](#2-rules)
3. [Reaching the agent](#3-reaching-the-agent)
4. [CLI](#4-cli)
5. [How it's built](#5-how-its-built)
6. [Open questions](#6-open-questions)
7. [Glossary](#7-glossary)

## 1. The loop

```mermaid
sequenceDiagram
    participant You
    participant Doc as markdown pane
    participant D as daemon
    participant A as agent
    You->>Doc: select text and ⌘↵, or ⌘-click a word
    You->>Doc: write, post
    Doc->>D: new thread c2
    D->>A: types c2 into the agent's pane, then Enter
    A->>A: edits the file
    A->>D: kanna doc reply c2 "…"
    A->>D: kanna doc resolve c2
    D->>Doc: c2 updated
```

General remarks that aren't about a passage go straight to the agent's chat, as now.

## 2. Rules

- **Preview only.** The markdown pane shows the file rendered and reloads when it changes; you don't edit in it. Comments are the only thing you add.
- **Commenting.** Select text and press ⌘↵ (or Comment), or ⌘-click a word to comment on just that word. ⌘-click always comments, even on a link; a plain click follows a link.
- **Threads live beside the file, not in it.** A thread has an ID (`c1`, `c2`, … per doc), the quoted passage, its messages, and whether it's open or resolved.
- **Anchors follow the text.** A comment remembers its quote with some text either side. When the file changes (the agent, you in another editor, git), it finds the quote again; if it can't, the thread is kept and marked **detached**, never dropped.
- **Resolved threads are hidden**, but kept and shown on request; resolving can be undone.
- **Not committed.** Threads are stored in `<repo>/.kanna/docs/`. Kanna doesn't commit them or change `.gitignore`; whether a team commits `.kanna/` is its own decision.

## 3. Reaching the agent

**A comment is typed into the agent's pane, as `kanna send` would, the moment you post it.** The same for every agent: Claude, Codex or anything else that reads its terminal. There's no button, no waiting and no per-agent connection.

- **Which pane:** the doc's agent, shown in the toolbar. It's the terminal that ran `kanna open md` for the doc, and you can pick another from the list.
- **What's typed:** one paste, then Enter. It names the thread, file and line, quotes the passage, gives your words, and says how to answer:

```
[kanna comment c2 · docs/specs/tab-container.md:18]
> the strip scrolls, keeping the active tab in view
Also say what ⌃Tab does when the strip has scrolled.
Reply: kanna doc reply c2 "…"   Resolve: kanna doc resolve c2
```

- **Replies you add to a thread** are sent the same way, under the same header.
- **Agents queue it themselves.** Claude Code and Codex hold input typed during a turn and take it next. If you have unsent text in the agent's prompt, the comment joins it; that's the price of keeping this simple, and Kanna doesn't try to detect it (v3 did, and it was never reliable).
- **No agent pane** (none chosen, or it's closed): posting still saves the thread, and the toolbar says it wasn't sent.

```mermaid
stateDiagram-v2
    [*] --> sent: you post
    sent --> replied: kanna doc reply
    replied --> sent: you reply again
    sent --> resolved: resolved
    replied --> resolved
    resolved --> sent: reopen
```

## 4. CLI

Inside a pane, the doc is found from the thread ID; elsewhere pass `--doc FILE`.

| Command | Does |
|---|---|
| `kanna doc comments [--all]` | List the doc's threads (open only, unless `--all`) |
| `kanna doc show cN` | One thread: quote, line, messages, state |
| `kanna doc reply cN TEXT` | Answer a thread |
| `kanna doc resolve cN` / `reopen cN` | Close or reopen a thread |
| `kanna doc comment FILE --quote TEXT TEXT` | Start a thread from the command line |
| `kanna doc agent FILE PANE` | Choose the doc's agent pane |

## 5. How it's built

```mermaid
flowchart LR
    subgraph app["Kanna.app"]
        V["markdown pane<br/>preview + thread panel"]
    end
    subgraph daemon["daemon"]
        S["doc store<br/>threads, anchors"]
        T["terminals"]
    end
    CLI["kanna doc reply / resolve<br/>(from the agent)"]
    V <-->|app link| S
    S -->|"paste + Enter"| T
    CLI --> S
    S --- F[("&lt;repo&gt;/.kanna/docs/")]
```

- **Threads live in the daemon**, beside the terminals they're typed into, so `reply` and `resolve` work the same with the app closed. The anchoring and thread logic come from v3's `kanna3-doc` crate, ported as `crates/kanna-doc`; v3's delivery gate and draft tracking are not ported.
- **Sending** uses the daemon's existing `send`: no new path into a terminal.
- **The app** adds the thread panel, highlighting and the comment box to the existing markdown pane, and gets thread changes over the app link like any other pane update.
- **Stored** in `<repo>/.kanna/docs/`, one file per doc, so threads survive restarts.

## 6. Open questions

1. Several comments posted quickly arrive as several messages. Should they be batched into one?

## 7. Glossary

| Term | Meaning |
|---|---|
| **Thread** | A comment and its replies, with an ID like `c2` |
| **Anchor** | What ties a comment to its passage: the quote and the text around it |
| **Detached** | A thread whose quote is no longer in the file |
| **Agent pane** | The terminal a doc's comments are typed into |
