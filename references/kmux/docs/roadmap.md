# Kmux - Roadmap

## Linux version

See the Linux question under [open questions](#open-questions).

## Feature comparison to cmux

Done: [research/cmux-comparison.md](research/cmux-comparison.md).
Deeper on simulator, API/CLI and layout: [research/cmux-sim-api-layout.md](research/cmux-sim-api-layout.md). (It corrects the comparison: cmux *does* embed the iOS simulator.)

## Client/Server approach research

Done, waiting on your answers: [research/client-server.md](research/client-server.md). It recommends a local server with one or more clients, and asks 6 questions only you can answer.

## Markdown viewer

Done: native markdown panes with back/forward and pinch zoom. Mermaid without a web view: [research/mermaid-renderers.md](research/mermaid-renderers.md) recommends merman.

## Layout persistence

Layouts come back after kmux restarts, with new terminals (decided in the spec, not built yet). Don't re-run commands from a session file kmux didn't write itself ([research/cmux-sim-api-layout.md](research/cmux-sim-api-layout.md)).

## Open questions

Each was researched on 2026-10-09; the findings are in the linked docs.

* look into improving webview access for agents. I think they should be able to access the webview directly and it's up to Apple to allow Safari's DOM or debug features be accessible by agents.
  * Apple won't let other apps attach to its web inspector, and Safari MCP only drives Safari, so kmux needs its own commands: `web.eval`, `web.tree`, `web.click`/`web.fill`, `web.wait`, `web.console`, screenshots via `snapshot`. [research/web-agent-access.md](research/web-agent-access.md)
  * Decide: agent access on by default per web pane? Private profile for agent panes? A kmux MCP server, or leave that to kanna?
* methods for agent to interact with iOS simulator
  * kmux can read the simulator's accessibility tree and inject input itself, with nothing installed in the simulator. Xcode 27 may break today's clicks (phase 0). Proposed `tap`, `swipe`, `type`, `key`, `button`, `wait`, `app` commands. [research/ios-agent-control.md](research/ios-agent-control.md)
  * Decide: points or fractions for coordinates? `kmux ios …` or shared verbs? Settle after every action?
* research further into cmux about simulator support and their api/cli, layout features
  * cmux runs the simulator in a separate process (a crash doesn't take the app down); its resumable event stream is worth copying; named layouts. Skip sidebars, tabs in panes, socket passwords. [research/cmux-sim-api-layout.md](research/cmux-sim-api-layout.md)
  * Decide: should panes opened over the socket stop taking focus? Named layouts in kmux or kanna?
* let's look at how we can improve our CLI without adding meaningless features.
  * Agents can do things but can't check them. Fix exit codes (always 0, a bug), add `read` and `wait`, make commands report what they did. [research/cli-improvements.md](research/cli-improvements.md)
  * Decide: should `open` keep your focus where it is? Visible screen only for a first `read`?
* we definitely want agents to be able to snapshot pane contents. probably a few different features here like a low-res full pane or high res ROIs.
  * Built, not merged: `kmux snapshot PANE [--max-size N] [--region x,y,w,h] [--text [--lines N]]` on branch `rq/pane-snapshot`. iOS image snapshots haven't been run yet.
* what are events/subs for in cmux?
  * For long-lived clients: sidebars, notification feeds, remote UIs. Not needed yet: record per-pane state (command finished, attention) and add a blocking `kmux wait`; add `subscribe` when a long-lived client exists. [research/cmux-events.md](research/cmux-events.md)
* likewise what's agent attention for?
  * Showing which pane needs you (an agent is blocked or done). Ghostty already reports notifications and the bell, and kmux drops them. Proposed: an `attention` state per pane, a ring, a Dock badge, `kmux notify`; kanna installs the agent hooks. [research/agent-attention.md](research/agent-attention.md)
  * Decide: does a bell count? Different looks for "needs input" and "done"? macOS notifications on by default?
* a really good client/server approach could be nice. we need to discuss this and what problem it solves.
  * See [research/client-server.md](research/client-server.md): waiting on your 6 answers.
* if ghostty can run on linux then we can (except for iOS simulator)
  * Ghostty runs on Linux, but the embedding API kmux uses doesn't yet. Now: about 2 days to make the core and CLI portable. Later: a headless server (2–3 weeks) or a GTK app (2–3 months). [research/linux.md](research/linux.md)
  * Decide: what is Linux for: desktop, headless agents and CI, or a server your Mac attaches to?
* an iPad or iPhone app would also be cool, but needs discussion.
  * Start as an "agent inbox" for the Mac's kmux: see which pane needs you, read it, reply. First `read`, `wait` and `notify` on the Mac (about a week), then a TestFlight app (about 4 weeks). [research/ipad-iphone.md](research/ipad-iphone.md)
  * Decide: just you or everyone? kmux or kanna? Is screen text enough at first?
* we should do a performance comparison with cmux (just quick and dirty)
  * kmux starts 6× lighter (38 vs 236 MB), 2.3× faster, at 1/5 the idle CPU, and is 20× smaller on disk. But it adds 17 MB per tab to cmux's 4.4: hidden tabs keep their GPU buffers, worth fixing. [research/cmux-perf.md](research/cmux-perf.md)
* we should add a build system for distribution
  * Developer ID signing and notarization, a DMG on GitHub Releases, a Homebrew cask, the CLI inside the app; Sparkle later. iOS panes need no weakened security. `scripts/package-kmux.sh` makes an unsigned local DMG. [research/distribution.md](research/distribution.md)
  * Decide: Apple Developer account? Apple Silicon only? Bundle ID `dev.kanna.kmux`?
