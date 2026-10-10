# Doc Markup

Comment on a markdown file in its pane, and the agent that opened it answers.

## Reaching the agent

- A comment is typed into the agent that opened the doc, as `kanna send` would, the moment you post it.
- Sent at once, never held. Claude Code and Codex queue input that arrives during a turn.
- No agent: posting still saves the thread, and the toolbar says it wasn't sent.

## How it's built

Threads live in the daemon, beside the terminals they're typed into, so `reply` and `resolve` work the same with the app closed.

| Part | Where |
|---|---|
| Threads | the daemon |
| Panel | the app |

> Comments are stored in `.kanna/docs/`, one file per doc.

The SENDME word is for the test.
