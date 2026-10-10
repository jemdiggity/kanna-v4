# Agent status signals: what Claude Code and Codex actually send

> 2026-10-10. Measured, for [Agent Status](../specs/agent-status.md). Claude Code 2.1.296 and Codex 0.162.0, each run in a pty against a local mock model with [`tools/agent-harness`](../../tools/agent-harness/README.md); no account or real model was used. Times are seconds from the agent's start.

## Contents

1. [In short](#1-in-short)
2. [How it was measured](#2-how-it-was-measured)
3. [Claude Code](#3-claude-code)
4. [Codex](#4-codex)
5. [Settings that control it](#5-settings-that-control-it)
6. [What it means for Agent Status](#6-what-it-means-for-agent-status)
7. [Not covered](#7-not-covered)

---

## 1. In short

| | Claude Code (Kanna's environment, defaults) | Codex (Kanna's environment, defaults) |
|---|---|---|
| Turn running | OSC 9;4;3 (indeterminate progress) from the start of the turn; title `◐ Claude Code` / `◑ Claude Code` every ~1 s | No progress at all. Title spinner `⠋ ⠋ \| work` every ~0.1 s, plus screen output |
| Turn ends | OSC 9;4;0 (progress cleared); title back to `✳ Claude Code`. **No bell, no notification** | OSC 9 `<first words of the answer>` **only when the pane is unfocused** (focus-out received); nothing when focused |
| Asks for approval | OSC 777 `notify;Claude Code;Claude needs your permission`, **6 s** after the prompt appears, focused or not. Progress stays at 9;4;3 the whole time | OSC 9 `Approval requested: <command>` at once, **only when unfocused**. Title blinks `[ . ] Action Required \| work` / `[ ! ] …` every 1 s, focused or not |
| Idle at the prompt | OSC 777 `notify;Claude Code;Claude is waiting for your input` **once, 60 s** after the turn ended, focused or not | Nothing, ever |
| Bell (BEL) | Never by default; only with `preferredNotifChannel: terminal_bell` (or `iterm2_with_bell`), and then in place of OSC 777 | Never by default in a Ghostty-like terminal; BEL instead of OSC 9 when the terminal isn't recognised (`TERM=xterm-256color`) or with `notification_method = "bel"` |
| Progress (OSC 9;4) | Yes, but only when `TERM_PROGRAM=ghostty` and `TERM_PROGRAM_VERSION` ≥ 1.2.0 (or iTerm ≥ 3.6.6) | Never (the binary has no OSC 9;4 at all) |
| OSC 777 | Yes (its "ghostty" channel) | Never |
| Focus reporting | Turns it on (`CSI ? 1004 h`); stops its approval blink when unfocused, but notifies either way | Turns it on and uses it to decide whether to notify |

So: **needs you can rely on OSC 9 / OSC 777 by default in Kanna's environment, but not on the bell**, and the two agents mean different things by a notification. **"Printed in the last 2 s" is wrong while an agent waits for approval**: both keep writing (Claude's blinking dot when focused; Codex's blinking title always).

## 2. How it was measured

Each run starts the agent with its own HOME (and `CLAUDE_CONFIG_DIR` / `CODEX_HOME`) pointed at the mock, then:

| Phase (mark) | What happens |
|---|---|
| `startup` | agent starts; the harness reports focus-in once |
| `plain` → `plain_end` | `PLAIN_TURN…` is sent; the mock waits 2 s, then streams 13 lines over ~6 s |
| `tool` → `asking` | `TOOL_TURN…` is sent; the mock asks to run `touch kanna-probe.txt`; the agent shows its approval prompt (Claude `--permission-mode default`; Codex `approval_policy = "on-request"` with `sandbox_permissions: "require_escalated"`) |
| `asking` → `approve` | 25 s at the approval prompt, then Enter (approve) |
| `idle` → `end` | 80 s at the prompt after the turn, then the agent is stopped |

| Axis | Values |
|---|---|
| Terminal environment | **kanna**: `TERM=xterm-ghostty COLORTERM=truecolor TERM_PROGRAM=ghostty TERM_PROGRAM_VERSION=1.3.2-dev+4ec3122` (what `kanna-keeper`'s `ghostty_env.rs` gives); **noprogram**: the same without `TERM_PROGRAM*`; **xterm**: `TERM=xterm-256color` only |
| Focus | **here**: focus-in once, kept; **away**: focus-out right after each prompt is sent (and after approving), focus-in before the next |
| Settings | Claude: default, `preferredNotifChannel: "terminal_bell"`. Codex: default, `[tui] notifications = true`, `notification_condition = "always"`, `+ notification_method = "bel"` |

The harness answers the agents' terminal queries as Ghostty would (DA1, cursor position, XTVERSION `ghostty 1.3.2`, kitty keyboard, DECRQM, OSC 10/11).

## 3. Claude Code

Every Claude run, by environment and setting (focus made no difference to any of these signals; `here` and `away` matched to within 0.1 s):

| Environment | Setting | Turn (plain, 9.8–18.6) | Approval prompt (shown 35.5) | Turn end after approval (62.9) | Idle (62.9–142.9) |
|---|---|---|---|---|---|
| kanna | default | `9.82 ESC]9;4;3;BEL` … `18.62 ESC]9;4;0;BEL` | `33.30 ESC]9;4;3;BEL`, `41.48 ESC]777;notify;Claude Code;Claude needs your permission BEL` | `62.90 ESC]9;4;0;BEL` | `122.89 ESC]777;notify;Claude Code;Claude is waiting for your input BEL` |
| kanna | terminal_bell | `9.85 ESC]9;4;3;BEL` … `18.62 ESC]9;4;0;BEL` | `33.35 ESC]9;4;3;BEL`, `41.42 BEL` | `62.90 ESC]9;4;0;BEL` | `122.89 BEL` |
| noprogram | default | nothing | `41.42 ESC]777;notify;Claude Code;Claude needs your permission BEL` | nothing | `122.86 ESC]777;…Claude is waiting for your input BEL` |
| noprogram | terminal_bell | nothing | `41.39 BEL` | nothing | `122.87 BEL` |
| xterm | default | nothing | **nothing** | nothing | **nothing** |
| xterm | terminal_bell | nothing | `41.09 BEL` | nothing | `122.57 BEL` |

Also in every run: `ESC]9;4;0;BEL` at startup and three times at exit (kanna only), and titles `ESC]0;✳ Claude Code BEL` at rest, `ESC]0;◐ Claude Code BEL` / `◑` alternating every ~0.96 s while a request is in flight, `ESC]0;BEL` (empty) at exit.

Why, from the 2.1.296 binary:

- **Notifications** go through one function: with `preferredNotifChannel` `auto` (the default) it picks the channel from the terminal, and `TERM=xterm-ghostty` is detected as `ghostty` before `TERM_PROGRAM` is looked at, so `auto` sends OSC 777 even without `TERM_PROGRAM`; `iTerm.app` → OSC 9; `kitty` → OSC 99; Apple Terminal → BEL (if its bell is on); anything else → nothing. Choices: `auto`, `iterm2` (OSC 9), `terminal_bell`, `iterm2_with_bell`, `kitty`, `ghostty` (OSC 777), `notifications_disabled`.
- **Permission prompt:** notified after 6 s with no input on the dialog (`aVt=6000`), every time a prompt is shown.
- **Idle:** one notification per finished turn, `messageIdleNotifThresholdMs` (default 60000) after it ended, if you haven't interacted since and no dialog is open. Not repeated.
- **Turn end:** no terminal notification. (Its `taskCompleteNotifEnabled` / `inputNeededNotifEnabled` are push notifications to the phone, not terminal ones.)
- **Progress:** `terminalProgressBarEnabled` (default true) and the terminal must be `TERM_PROGRAM=ghostty` with `TERM_PROGRAM_VERSION` ≥ 1.2.0, or `iTerm.app` ≥ 3.6.6 (or ConEmu). It is set to indeterminate when a request starts and cleared when the turn ends; **it stays set while the approval prompt waits**.
- **Title:** on unless `CLAUDE_CODE_DISABLE_TERMINAL_TITLE`.

## 4. Codex

| Environment | Setting | Focus | Turn ends (plain) | Approval prompt | Turn ends (after approval) | Idle 80 s |
|---|---|---|---|---|---|---|
| kanna | default | here | nothing | nothing | nothing | nothing |
| kanna | default | away | `26.21 ESC]9;PLAIN_DONE: a scripted answer … Line...BEL` | `42.39 ESC]9;Approval requested: /bin/zsh -lc 'touch kanna-p...BEL` | `69.74 ESC]9;TOOL_DONE: the probe file was created.BEL` | nothing |
| kanna | `notifications = true` | here / away | same as default (nothing / OSC 9 ×3) | | | nothing |
| noprogram | default | here | nothing | nothing | nothing | nothing |
| noprogram | default | away | `31.61 ESC]9;PLAIN_DONE…BEL` | `47.89 ESC]9;Approval requested: …BEL` | `75.24 ESC]9;TOOL_DONE…BEL` | nothing |
| kanna | `notification_condition = "always"` | here | `26.02 ESC]9;PLAIN_DONE…BEL` | `42.09 ESC]9;Approval requested: …BEL` | `69.42 ESC]9;TOOL_DONE…BEL` | nothing |
| kanna | `always` + `notification_method = "bel"` | here / away | `18.44 BEL` | `34.58 BEL` | `61.90 BEL` | nothing |
| xterm | default | here | nothing | nothing | nothing | nothing |
| xterm | default | away | `18.28 BEL` | `34.33 BEL` | `61.64 BEL` | nothing |
| xterm | `always` | here / away | `18.43 BEL` | `34.54 BEL` | `61.88 BEL` | nothing |

Titles, in every run: `ESC]0;work BEL` at rest (the folder name); while working a braille spinner every ~0.1 s, `ESC]0;⠋ ⠋ | work BEL`; at the approval prompt `ESC]0;[ . ] Action Required | work BEL` and `ESC]0;[ ! ] Action Required | work BEL` alternating every 1.0 s until answered — **the only writes during approval** (no screen output). No OSC 9;4 and no OSC 777 in any run.

Why, from 0.162.0's config (`[tui]` in `config.toml`): `notifications` (default **on**; a list of event names also allowed: `agent-turn-complete`, `approval-requested`, `plan-mode-prompt`, `async-question`), `notification_method` `auto` | `osc9` | `bel` (auto: OSC 9 when the terminal is recognised as one that shows it, Ghostty included, detected from `TERM` too; else BEL), `notification_condition` `unfocused` (default) | `always`. "Unfocused" comes from the terminal's focus reports (Codex turns them on with `CSI ? 1004 h`); every run reported focus-in at startup, so what Codex assumes before any report was not measured. The OSC 9 text is the start of the answer, cut at ~150 characters with `...`, or `Approval requested: <command>` cut at ~30.

## 5. Settings that control it

| Agent | Setting | Where | Default | Effect seen |
|---|---|---|---|---|
| Claude | `preferredNotifChannel` | `~/.claude/settings.json` (or `/config` → Notifications) | `auto` | `auto` in Kanna = OSC 777; `terminal_bell` = BEL instead; `notifications_disabled` = nothing |
| Claude | `messageIdleNotifThresholdMs` | `~/.claude.json` | 60000 | delay of the idle notification |
| Claude | `terminalProgressBarEnabled` | `/config` → Terminal progress bar | true | OSC 9;4 (still needs `TERM_PROGRAM=ghostty` ≥ 1.2.0) |
| Claude | `CLAUDE_CODE_DISABLE_TERMINAL_TITLE` | env | unset | title off |
| Codex | `[tui] notifications` | `$CODEX_HOME/config.toml` | true | turn end and approval notifications |
| Codex | `[tui] notification_condition` | same | `unfocused` | `always` notifies even when focused |
| Codex | `[tui] notification_method` | same | `auto` | `bel` = BEL instead of OSC 9 |
| Codex | `[tui] terminal_title` | same | spinner + folder | the title items |

## 6. What it means for Agent Status

The spec's rules, checked against the evidence:

| Spec rule | Holds? | Evidence |
|---|---|---|
| "working = printed in the last 2 s" | **No, not on its own.** Both agents keep writing while they wait for approval: Claude redraws a blinking `⏺` every ~1 s while focused (40 screen writes in 25 s), Codex rewrites its title every 1 s whether focused or not. At an idle prompt both are silent (Claude: 2 writes in 80 s, both the idle notification; Codex: none after the turn) | §3, §4 |
| "or reports progress (OSC 9;4)" | **Claude only**, and only because Kanna sets `TERM_PROGRAM=ghostty` + a version ≥ 1.2.0. Codex never sends it. Claude's progress stays set during the approval wait | §3 |
| "needs you = bell or OSC 9/777" | **Yes for OSC 9/777 by default; the bell is never sent by default** in Kanna's environment. Claude: OSC 777 6 s into an approval prompt and 60 s after a turn. Codex: OSC 9 at once on approval and at every turn end, but only when its pane is unfocused | §3, §4 |
| "Claude Code and Codex ring the bell or notify when they want input (each has a setting for it)" | Partly. They notify, by default, but not the same way: Claude never at turn end, Codex never when focused | §1 |

Recommendations:

1. **Needs you:** keep "bell, OSC 9 or OSC 777 since you last looked". It works by default with Kanna's environment and needs no setting. Keep the bell for users who choose `terminal_bell` / `notification_method = "bel"` and for other programs.
2. **Kanna must deliver focus events to the pane (`CSI I` / `CSI O` when mode 1004 is on)** and should send focus-out to every pane that is not the focused one. Codex notifies only on focus-out; without it, Codex never says it needs you. Ghostty does this when a surface loses focus; check that Kanna's pane focus, window focus and app deactivation all reach it.
3. **Working:** count only writes that change the screen, not title-only writes (OSC 0/1/2 and the other OSCs). That makes Codex's approval wait (title blink only) fall to idle after 2 s instead of reading as working. Keep "or progress is set" for Claude; with needs-you winning, Claude's progress during an approval wait is covered by its OSC 777 after 6 s.
4. **Turn end is not uniform:** Codex turns every unfocused turn end into a notification (so "needs you"); Claude only says so 60 s later. If the Desk should show "done, your turn" the same for both, add: **progress going from set to cleared while you're not looking → needs you**. That is a terminal signal (Claude's OSC 9;4;0), not screen parsing, and it makes Claude match Codex. Without it, a finished Claude turn shows idle for 60 s, then needs you.
5. **Kanna's environment is already right:** `TERM=xterm-ghostty`, `TERM_PROGRAM=ghostty`, `TERM_PROGRAM_VERSION=<ghostty version ≥ 1.2.0>` are what turn on Claude's progress and OSC 777. Keep `TERM_PROGRAM_VERSION` ≥ 1.2.0 (Claude reads it with semver; `1.3.2-dev+4ec3122` works). Kanna should not change the agents' own config; nothing needs it.
6. **Looking clears needs you:** a pane that is on screen but not focused gets Codex's notifications (it's unfocused) and, by the current rule, is already "looked at". Decide whether on screen in the key window is enough, or whether it must be the focused pane; the evidence suggests the focused pane, to match how Codex decides.
7. **OSC 9 text is the model's answer.** An answer that starts with `4;` would read as OSC 9;4 progress. Rare; worth knowing when parsing.

## 7. Not covered

- Real models: the mock answers evenly, after 2 s of silence. A real model's longer silent "thinking" is where "printed in the last 2 s" could fail; Claude's progress covers it, and Codex redraws its `Working (Ns • esc to interrupt)` status line on screen with every spinner frame (all 113 writes in the 3 s before the first token changed the screen, not just the title), so rule 3 keeps it working.
- Claude's auto permission mode (its new default) and plan-mode / question prompts (Codex `plan-mode-prompt`, `async-question`) were not exercised.
- Claude's feature flags come from a server; with `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` and no login they are at their built-in defaults (e.g. `showStatusInTerminalTab` is behind one and was off).
- `iterm2_with_bell`, `kitty` channels and Codex in tmux were not run.
