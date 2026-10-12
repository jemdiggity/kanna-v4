---
name: kanna-explain
description: Use in a Kanna terminal ($KANNA_PANE is set) when the user asks how some code works, wants to understand code you wrote or changed, or seems to be losing track of the code.
---

# Kanna explain

Help the user keep a mental model of the code: show them the code you're talking about, one part at a time, with a comment on it.

## Pick the way that fits

- **How a part works:** comment on the code itself.

  ```sh
  kanna comment src/fdpass.rs:50 "Waits for a message and takes the terminals that came with it."
  ```

  The file opens in a code tab beside you (or its tab is selected), those lines are tinted, the rest dimmed, and the comment shows just above them. `FILE:LINE` takes the whole function, loop or block that line opens or sits in; `FILE:FIRST-LAST` takes just those lines, for a few lines inside a long function. A new comment in the same file replaces the last one.

- **How the parts fit together:** a markdown doc with a diagram (a sequence diagram, or a layer diagram of the processes), then comment on it the same way.

  ```sh
  kanna comment docs/how-it-works.md:12 "The keeper owns the terminals; the daemon only borrows them."
  ```

  On a markdown file the comment is a thread beside the doc, on the block at that line; the user's replies in it come to you as `[kanna comment …]` messages (see the `kanna-markup` skill). A line on an image or a diagram puts the thread on the text just above it.

Often both: the doc for the big picture first, then the code.

## One comment at a time

1. Comment on one part.
2. Explain it in the chat. Keep the comment itself to the one thing to remember about that part.
3. Stop, and ask the user to tell you when to go on. Never comment on several parts in one turn.

When the user asks a question, answer it by commenting on the code that answers it.

## At the end

Offer to leave the tabs open or close them, and do what the user picks (`kanna close PANE` for each code tab you opened; `kanna comment` printed their IDs).
