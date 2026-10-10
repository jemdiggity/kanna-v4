---
name: kanna-doc
description: Use when a message starting with "[kanna comment cN · FILE:LINE]" arrives, or when the user asks to review a markdown doc together in Kanna.
---

# Kanna doc comments

The user reviews markdown docs in Kanna and comments on passages. Each comment is typed into your terminal as:

```
[kanna comment c2 · docs/specs/tab-container.md:18]
> the passage they selected
what they want
```

`c2` is the comment's thread in that doc. Later messages with the same header are the user's replies in that thread.

## Answering a comment

1. Read the passage in the file (the line is where it is now) and do what the comment asks: change the doc, or answer the question.
2. Reply in the thread, briefly, saying what you changed or answering:
   `kanna-doc reply c2 "Added: ⌃Tab scrolls the next tab into view."`
3. If your change settles it, resolve it: `kanna-doc resolve c2`. If it needs the user's decision, reply and leave it open.

Reply in the thread, not only in the chat: the user reads replies beside the doc.

## Other commands

| Command | Does |
|---|---|
| `kanna-doc open FILE` | Show FILE with its comments beside your pane. Comments on it come to you. |
| `kanna-doc comments [--all]` | The doc's open comments (`--all`: resolved too) |
| `kanna-doc show cN` | One thread: quote, line, every message |
| `kanna-doc reopen cN` | Reopen a resolved thread |

Run them from your own terminal: the doc is found from the thread ID and the pane you're in. Pass `--doc FILE` if that's ambiguous.

`kanna-doc` is the prototype of `kanna doc` (Kanna v4's `prototypes/doc-markup/kanna-doc`); once Kanna has `kanna doc`, use that with the same subcommands.
