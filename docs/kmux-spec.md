# kmux — Specification

> **Status:** v0.5 · **Last updated:** 2026-10-08
>
> **Reference model:** [`reference/kmux/index.html`](../reference/kmux/index.html). Open it in a browser and try it.
> The model is the source of truth for kmux's design and behaviour. This document summarises what the model shows and lists what it doesn't answer yet. If the two disagree, the model wins, and this document should be fixed.
>
> **Related:** [kanna spec](kanna-spec.md), the CLI that drives kmux.

## Table of Contents

1. [Summary](#1-summary)
2. [What the Model Captures](#2-what-the-model-captures)
3. [Windows, Tabs and Layout](#3-windows-tabs-and-layout)
4. [Panes](#4-panes)
5. [Menus and Shortcuts](#5-menus-and-shortcuts)
6. [Moving Windows, Tabs and Panes](#6-moving-windows-tabs-and-panes)
7. [Control Protocol](#7-control-protocol)
8. [Decisions and Open Questions](#8-decisions-and-open-questions)
9. [Glossary](#9-glossary)

---

## 1. Summary

kmux is a native macOS **mux**. It has one or more windows of tabs, and each tab is split into **panes** that show Ghostty terminals, web pages or the iOS Simulator. Panes have no chrome. Everything is done through menus, shortcuts and dragging, or by other programs through the **control protocol**.

UI actions and protocol requests go through the same core, so a click and a `kanna` command always behave the same way.

![A kmux window with a terminal running a dev server, a web pane showing the site and an iOS pane showing the app](img/kmux/layout.png)

```mermaid
flowchart LR
    H["Mouse, keyboard, menus"] --> CORE["kmux core<br/>windows · tabs · layout · panes"]
    C["kanna / scripts / agents"] -->|"control protocol"| CORE
    CORE --> T["term panes (Ghostty)"]
    CORE --> W["web panes"]
    CORE --> I["ios panes (Simulator)"]
```

---

## 2. What the Model Captures

| Characteristic | Captured? | Notes |
|----------------|:---------:|-------|
| Look: windows, tabs, chromeless panes, menus | ✅ | |
| Layout behaviour: splits, sizes, dragging, moving | ✅ | Includes moving between windows. |
| Pane lifecycle and error states | ✅ | |
| Shortcuts and menus | ✅ | The browser keeps ⌘N, ⌘T, ⌘W, ⇧⌘W and ⌘\`, so the model also accepts ⌥ in place of ⌘. |
| Control-protocol commands and replies | ✅ | The model's console (marked **MOCKUP ONLY**) stands in for kanna. It is not part of kmux. |
| Real terminals, web views and simulators | ❌ | Simulated. See [section 8](#8-decisions-and-open-questions). |
| Transport (socket), persistence, performance | ❌ | See [section 8](#8-decisions-and-open-questions). |

---

## 3. Windows, Tabs and Layout

### 3.1 Structure

kmux has any number of **windows**. A window holds **tabs**. Each tab holds a tree of **splits** (`row` = side by side, `column` = stacked) with panes at the leaves.

```mermaid
flowchart TD
    APP["kmux"] --> W1["Window"]
    APP --> W2["Window"]
    W1 --> T1["Tab"]
    W1 --> T2["Tab"]
    T1 --> R["row"]
    R --> P1["pane · 2/3"]
    R --> C["column · 1/3"]
    C --> P2["pane · 2/3"]
    C --> P3["pane · 1/3"]
```

![Two kmux windows. The front window is the key window and has two tabs.](img/kmux/windows.png)

- **Key window:** one window is the key (front) window. Shortcuts, menus and protocol requests without a `window` act on it. Clicking a window makes it key.
- **Focus:** each window remembers its focused pane and zoom state, and each tab remembers its last focused pane.
- **Titles:** a tab's title lists its pane names, e.g. `server · site`. A window's title is its active tab's title.
- **Empty containers close:** closing a tab's last pane closes the tab, and closing a window's last tab closes the window. With no windows left, kmux offers to open a new one.
- **Closing a tab or window** closes all of its panes.
- **New windows** cascade down and to the right of the key window.

### 3.2 Sizes

Every size is a **fraction of its parent split**. The fractions in a split always add up to 1, and they stay proportional when the window is resized.

| Action | Rule |
|--------|------|
| New split | The new pane gets `size` (default **1/2**) of the focused pane's space. |
| Split direction | `right` or `down` when given. Otherwise (`auto`), side by side if the focused pane is wider than it is tall, stacked if not. |
| Same-direction parent | The new pane becomes a sibling in that split, not a nested split, so three "split right"s give three columns. |
| New pane focus | The new pane takes focus, and its window becomes key. |
| Close | The siblings share the freed space in proportion to their sizes. A split left with one child disappears. |
| Resize | The pane gets the new fraction, and its siblings share the rest in proportion to their sizes. |
| Divider drag | Snaps to ¼, ⅓, ½, ⅔ or ¾ when within 1.5% of one. A tooltip shows both fractions, e.g. `2/3 \| 1/3`. Panes can't be dragged below 80 px. |

![Dragging a divider: the tooltip shows the two panes' shares](img/kmux/divider-drag.png)

### 3.3 Arrange

`arrange` replaces a window's active tab layout with a given tree (see [7.2](#72-layout-trees)):

- The tree can name panes from any window, and those panes move into this tab.
- Children without a size share whatever space is left. If every child has a size and they add up to less than 1, they are scaled up to fill the space.
- If the sizes add up to more than 1, the request is rejected and nothing changes.
- Panes in the tab that the tree doesn't mention move to a new tab called `unarranged`.

---

## 4. Panes

### 4.1 No chrome

- Panes have no header, title or border.
- A **✕** close button appears in the top-right corner while the pointer is over the pane.
- The focused pane has a thin accent outline, shown only in the key window and only when the tab has more than one pane.
- Errors and exits are shown inside the pane itself (see below).

![No chrome: an exited terminal, a web pane waiting for its server and an iOS pane that failed to start](img/kmux/pane-states.png)

### 4.2 Types

| Type | Shows | Behaviour in the model |
|------|-------|------------------------|
| `term` | A shell or command, in a Ghostty terminal | Runs `cmd` if given. Ctrl+C interrupts. `exit [code]` moves the pane to **exited** and shows the code. |
| `web` | A web page | No address bar. **Open URL** (⌘L) shows a floating address field. A `localhost` URL whose server isn't up shows "Waiting for …" and loads once the server responds. |
| `ios` | An app in the native iOS Simulator | Shows "Booting \<device\>…", then the app. Clicks are forwarded to the simulator. An unknown device fails and lists the available devices. |

![Open URL (⌘L) on a web pane](img/kmux/open-url.png)

### 4.3 Lifecycle

```mermaid
stateDiagram-v2
    [*] --> starting : open
    starting --> running
    starting --> failed
    running --> exited : process ended
    exited --> starting : restart
    failed --> starting : restart
    running --> closed : close
    exited --> closed : close
    failed --> closed : close
    closed --> [*]
```

`open`, `restart` and `navigate` wait until the pane is **running** or **failed** before replying, unless the request sets `wait: false`.

---

## 5. Menus and Shortcuts

The menu bar has **Pane**, **View** and **Window** menus. Right-clicking a pane opens the Pane menu for that pane. Right-clicking a tab offers **Move tab to new window** and **Close tab**.

| Action | Shortcut | Where in the UI |
|--------|----------|-----------------|
| Split right (new terminal) | ⌘D | Pane menu |
| Split down (new terminal) | ⇧⌘D | Pane menu |
| New web / iOS pane right or below | — | Pane menu |
| Next / previous pane | ⌘] / ⌘[ | Pane menu |
| Open URL (web panes) | ⌘L | Pane menu |
| Zoom / unzoom | ⇧⌘↩ | Pane menu |
| Move pane to new window | — | Pane menu |
| Restart | ⌘R | Pane menu |
| Close pane | ⌘W | Pane menu, hover ✕ |
| New tab | ⌘T | View menu, **+** |
| Next / previous tab | ⇧⌘] / ⇧⌘[ | View menu |
| New window | ⌘N | Window menu |
| Close window | ⇧⌘W | Window menu, red title-bar button |
| Next / previous window | ⌘\` / ⇧⌘\` | Window menu, or pick a window from the list in the Window menu |
| Move tab to new window | — | Window menu, tab right-click |

![The pane menu, opened by right-clicking a pane](img/kmux/pane-menu.png)

- Pane order is reading order: left to right, top to bottom. Pane, tab and window navigation all wrap around.
- Moving to a pane also moves keyboard input to it.
- Zoom makes one pane fill the tab. Moving focus to another pane cancels it.

---

## 6. Moving Windows, Tabs and Panes

**Windows:** drag a window by its title bar, and resize it from its bottom-right corner.

**Tabs:** drag a tab:

| Drop on | Result |
|---------|--------|
| A position in any window's tab bar | The tab moves there. A marker shows where it will land. |
| The desktop (outside every window) | The tab moves to a new window at that spot. |

![Dragging a tab onto the desktop to make a new window](img/kmux/tab-to-new-window.png)

**Panes:** hold ⌘ and drag a pane. This works within a window and between windows. A highlight shows where it will land:

| Drop on | Result |
|---------|--------|
| Near an edge of another pane | The pane docks on that side and takes half of the other pane's space. The nearest edge wins. |
| The centre of another pane (the middle 40% across and down) | The two panes swap places. |
| A tab | The pane moves to that tab as a new column. All columns then share the width equally. |
| A window's **+** | The pane moves to a new tab in that window. |
| The desktop | The pane moves to a new window at that spot. |

When a move leaves a tab or window empty, it closes ([3.1](#31-structure)).

**Docking on an edge:** dragging `phone` near the left edge of `server` highlights the left half.

![Dragging a pane near another pane's left edge](img/kmux/drop-edge.png)

**Swapping:** dragging it to the centre of `server` highlights the whole pane.

![Dragging a pane to another pane's centre](img/kmux/drop-swap.png)

**Moving to a new tab:** dragging it onto **+** highlights the button.

![Dragging a pane onto the + button](img/kmux/drop-new-tab.png)

---

## 7. Control Protocol

Each request is `{ id, cmd, args }`. Each reply is `{ id, ok: true, … }` or `{ id, ok: false, error: { code, message } }`.

- A pane can be referred to by its ID (`p2`) or its name (`site`). Tabs (`t1`) and windows (`w1`) are referred to by ID.
- IDs are never reused while kmux is running.
- Wherever `window` appears, it can be a window ID or `new`. Leaving it out means the key window.

### 7.1 Commands

| `cmd` | `args` | Notes |
|-------|--------|-------|
| `capabilities` | — | Pane types and features. |
| `open` | `type`, `url` / `cmd` / `cwd` / `app` / `device`, `name?`, `split?` (`right` / `down` / `auto`), `size?`, `tab?`, `window?`, `wait?` | See [3.2](#32-sizes). The reply includes the pane's window. |
| `arrange` | `layout` (tree), `window?` | See [3.3](#33-arrange). |
| `move` | `pane`, plus one of: `to` + `side` (`left` / `right` / `top` / `bottom` / `swap`); `tab` (ID, or `new` with optional `window`); `window` (ID or `new`) | See [section 6](#6-moving-windows-tabs-and-panes). Moving to a window adds the pane to that window's active tab. |
| `move-tab` | `tab`, `window?`, `index?` | Reorders a tab or moves it to another window, or to a new one. |
| `resize` | `pane`, `size` | Fails if the pane fills its tab. |
| `list` | — | Windows (with the key window marked), their tabs and layout trees, and all panes. |
| `focus` | one of `pane`, `tab`, `window` | Brings it to the front and makes its window key. |
| `zoom` | `pane` | Toggles zoom. |
| `close` | one of `pane`, `tab`, `window` | Closes it and everything inside it. |
| `restart` | `pane` | |
| `send` | `pane`, `text` | `term` panes only. Runs `text` as a typed line. |
| `navigate` | `pane`, `url` | `web` panes only. |

### 7.2 Layout trees

```json
{ "split": "row", "children": [
    { "pane": "server", "size": "2/3" },
    { "split": "column", "children": [ { "pane": "site" }, { "pane": "phone", "size": "1/3" } ] }
] }
```

A size can be written as a fraction (`"1/3"`), a percentage (`"25%"`) or a decimal (`0.25`). `list` and replies always use the simplest fraction, e.g. `"1/3"`.

### 7.3 Errors

| `code` | When |
|--------|------|
| `bad_request` | Unknown command or argument, invalid fraction, or resizing a pane that fills its tab. |
| `not_found` | Unknown pane, tab or window. |
| `name_taken` | Pane name already in use. |
| `wrong_type` | `send` to a non-terminal pane, or `navigate` to a non-web pane. |
| `layout_invalid` | Malformed tree, a pane listed twice, or sizes adding up to more than 1. |
| `start_failed` | The pane went to **failed**. The message says why. |

---

## 8. Decisions and Open Questions

### 8.1 Decided

| Topic | Decision |
|-------|----------|
| Platform | A native macOS app (AppKit). |
| Terminal | Ghostty (GhosttyKit), reusing kanna-v3's build pipeline and terminal view. Start from **upstream Ghostty**: kanna-v3's fork existed to stream terminals from a daemon, and kmux has no persisted terminals for now. |
| iOS pane | The native iOS Simulator. **Deferred:** not part of the first native builds. |
| Transport | A **persistent connection** over the Unix socket: a client keeps one connection open and sends many requests over it. |
| Process model | kmux is a single app that runs its own terminals and listens on the control socket itself. Later, we fork our own mux and move to a kanna-v3-style app-plus-daemon. |
| Reuse | Copy the kanna-v3 pieces kmux needs and trim them, without depending on v3. |
| kanna CLI | Rust. |
| Multiple windows | Required. Modelled in [3.1](#31-structure), [section 6](#6-moving-windows-tabs-and-panes) and [section 7](#7-control-protocol). |

### 8.2 Open

| # | Question | Needs |
|---|----------|-------|
| 1 | **iOS pane placement (deferred):** mirror the Simulator's screen into the pane, or keep the real Simulator window positioned over it? | A native prototype, once the Simulator is back in scope. |
| 2 | **Transport details:** framing (newline-delimited JSON, or length-prefixed frames as in kanna-v3), the socket path (`~/Library/Application Support/kmux/kmux.sock` is assumed) and permissions. | Decision during the native scaffold. |
| 3 | **Events:** can clients subscribe to changes (pane exited, URL changed)? | Decision, then extend the model. |
| 4 | **Pane identity:** without chrome, is the tab title enough to tell panes apart, or should the name show on hover? | Your call, then try it in the model. |
| 5 | **Tab commands:** should the protocol let clients rename tabs? | Decision. |
| 6 | **Persistence:** should windows and layouts survive an app restart? | Decision. This ties into the later daemon. |
| 7 | **Web navigation:** back/forward history and keyboard shortcuts? | Your call. |
| 8 | **Ghostty keybindings:** should kmux shortcuts come from the user's Ghostty config, as in kanna-v3, or be fixed? | Decision. |
| 9 | **Performance:** targets for pane start time, input latency and memory per pane. kanna-v3 aimed for under 50 ms of typing latency. | A benchmark utility (the rdd "performance reference"). |

---

## 9. Glossary

| Term | Meaning |
|------|---------|
| **Mux (multiplexer)** | An app that shows several separate sessions in one window. |
| **Chrome** | The UI around content, such as title bars, headers and borders. kmux panes have none. |
| **Key window** | The front window. It receives shortcuts and requests that don't name a window. |
| **Pane** | One area of a tab that shows one thing. |
| **Split** | A group of panes side by side (`row`) or stacked (`column`). |
| **Layout tree** | The nested splits and panes in a tab. |
| **Zoom** | Temporarily showing one pane over the whole tab. |
| **Control protocol** | The JSON requests that other programs use to drive kmux. |
| **Reference model** | The mockup in `reference/kmux/` that defines how kmux should look and behave. |
| **Ghostty / GhosttyKit** | A fast terminal emulator, and the library build of it that kmux embeds. |
| **iOS Simulator** | Apple's tool, part of Xcode, for running iPhone apps on a Mac. |
| **Daemon** | A background process that keeps running when the app quits. kmux may move terminals into one later. |
