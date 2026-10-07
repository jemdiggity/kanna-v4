---
name: reference-driven-design
description: Always load.
---

# Reference-driven design
Instead of endless specifications that inevitably drift from the actual codebase, we make simple mockups (static html or interactive web app) or prototypes (a non-performant version in python) or a utility that sets the standard for performance that capture the characteristics (design, behaviour, or performance) of the design and we use that as a long-living reference for the product.

## How to know what we're building

1. Start as high-level as possible.
1. Make the model (mockup, prototype, etc)
1. Iterate with the user until the meaningful characteristcs (design/behaviour/performance) is captured.
1. Create a brief specification doc that covers behaviour etc captured by the model. Items not covered by the model need to be determined with the user or indicate the model itself is insufficient.
1. Iterate with the user to flesh out the specification for items that are unclear.

## How to build

Don't build A, then B, then C and hope it all works when you plug them together. It never does. If there's one thing you learn in engineering school is that integration is the painful step, so make it the first step.

Start high level. Scaffold building blocks. Define interfaces and abstractions. Mock internals. Connect things top down. Make meaningful tests. Take babysteps. Add the minimal next step towards implementing the model's characteristics. Commit after each step. As characterictics span building blocks, each step should be implementing support for the feature across building blocks.

Iterate this process while comparing behaviour against the reference model.

