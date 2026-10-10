---
name: spec-docs
description: Use when the user asks to draft, write, or update a spec, design doc, or technical specification.
---

# Spec Docs
Write specification documents in plain english.
Include non-common technical terms in a glossary.
Include a table of contents.
Structure the document so, at a glance, the user can see the high level items and can dig further in for the details.
Make use of graphics. Use common image generation tools (mermaid etc) to include diagrams showing relationships, structure, states etc.
Keep a spec short: one feature per spec, in its own file next to its mockup, not a long section inside a big document. Link to it from the big document.
Don't describe what something looks like (colours, sizes, layout, styling) in words. That's what pictures are for: put in a screenshot of the mockup, and keep the words for behaviour, rules and interfaces, the things a picture can't show. When you edit a spec that already describes looks in words, replace the description with the picture; don't amend it.
