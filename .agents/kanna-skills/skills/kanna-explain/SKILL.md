---
name: kanna-explain
description: Use in a Kanna terminal ($KANNA_PANE is set) when the user asks how some code works, wants to understand code you wrote or changed, or seems to be losing track of the code.
---

# Kanna explain

Explain code where it is, not in the chat: the explanation goes in comments on the code and in diagrams, and the chat stays a line or two.

## Code: comment on it

```sh
kanna comment src/fdpass.rs:50 "Waits for a message and takes the terminals that came with it."
```

The file opens in a code tab beside you, those lines tinted and the rest dimmed, the comment just above them. `FILE:LINE` takes the whole function or block at that line; `FILE:FIRST-LAST` takes just those lines. A new comment in the same file replaces the last one.

- **The comment is the explanation.** Short and plain: what this part does and the one thing to remember about it.
- **Small parts.** A function, a match, a loop; not a whole file.

## The big picture: a temporary markdown doc

For how parts fit together (object hierarchies, class or layer stacks, who calls whom, how data flows), write a short markdown doc with a diagram in `$TMPDIR/kanna-explain/`, never in the repo, and comment on it:

```sh
kanna comment "$TMPDIR/kanna-explain/backlog.md:3" "The daemon owns the backlog; the app only shows it."
```

- **A diagram, not prose:** a mermaid `classDiagram` for a hierarchy, a `sequenceDiagram` for a flow, a layer diagram for a stack (the `layer-diagrams` skill), a table for a set of parallel things. A heading and the diagram; at most a line of text under it.
- **Big picture first,** then the code it points to.

## One part at a time

1. Comment on one part (code or doc).
2. Focus the pane `kanna comment` printed: `kanna focus p129`.
3. In the chat, one or two lines at most: where you are and "tell me when to go on". Don't repeat the comment, don't explain in the chat.
4. Stop and wait. Never comment on several parts in one turn.

When the user asks a question, answer it with a comment on the code or doc that answers it, and a line in the chat.

## At the end

Offer to leave the tabs open or close them, and do what the user picks (`kanna close PANE` for each tab `kanna comment` opened). Delete the temporary docs.
