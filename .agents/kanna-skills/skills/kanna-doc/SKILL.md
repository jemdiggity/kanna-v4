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

A comment on an image says which image and which part of it, and where a copy with that part boxed in red is:

```
[kanna comment c4 · docs/specs/desk.md:9]
> image desk.png, the box from 5%,20% to 30%,50% (left,top to right,bottom)
> marked up: .kanna/docs/marks/docs__specs__desk.md-c4.png
The agent header needs the branch.
```

A comment on a web page (an HTML mockup) names the part of the page, by CSS path and text, the text selected in it (`the text "…" in PATH`), or a box drawn on it, and a screenshot with it boxed:

```
[kanna comment c1 · docs/mockups/desk.html:149]
> the page at #picture,needs, element #win > div:nth-of-type(1) "Kanna Desk · Doc markup reference model"
> box 0,0 to 1220,44 px in a 1220×816 view, scrolled to 0,0
> marked up: .kanna/docs/marks/docs__mockups__desk.html-c1.png
Make the title bar quieter.
```

The line is where that text is in the HTML, when it's found. Open the marked-up copy (it's relative to the repo root) to see what they mean before you answer.

## Answering a comment

1. Read the passage in the file (the line is where it is now) and do what the comment asks: change the doc, or answer the question.
2. Reply in the thread, briefly, saying what you changed or answering:
   `kanna-doc reply c2 "Added: ⌃Tab scrolls the next tab into view."`
3. If your change settles it, resolve it: `kanna-doc resolve c2`. If it needs the user's decision, reply and leave it open.

Reply in the thread, not only in the chat: the user reads replies beside the doc.

## Other commands

| Command | Does |
|---|---|
| `kanna markup FILE.md`, `kanna markup FILE.html` | Show a markdown file, or a local page (a mockup), in a Kanna pane with its comments. Comments on it come to you. (`kanna open md` and `kanna open web` show the doc without comments; `kanna markup PANE` turns them on in one already open.) |
| `kanna-doc open FILE` | The same in the reference model's web pane |
| `kanna-doc comments [--all]` | The doc's open comments (`--all`: resolved too) |
| `kanna-doc show cN` | One thread: quote, line, every message |
| `kanna-doc reopen cN` | Reopen a resolved thread |

Run them from your own terminal: the doc is found from the thread ID and the pane you're in. Pass `--doc FILE` if that's ambiguous.

`kanna-doc` is the prototype of `kanna doc` (Kanna v4's `prototypes/doc-markup/kanna-doc`); once Kanna has `kanna doc`, use that with the same subcommands.
