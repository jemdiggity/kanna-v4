# Build Speed

**Status:** built · **Builds on:** [build channels](build-channels.md) · **Why:** every task's agent runs `scripts/build.sh` in a fresh worktree, and each worktree has its own `target/`, so cargo and SwiftPM used to start cold every time

## 1. What a fresh worktree borrows

```mermaid
flowchart LR
    subgraph M["main checkout's target/"]
        S["ghostty-source/<br/>(the fork at the pinned commit)"]
        G["ghosttykit/, ghostty-vt/HOST"]
        R["release/<br/>(cargo: ~270 dependencies)"]
        K["kanna-build/<br/>repositories/, checkouts/,<br/>workspace-state.json"]
    end
    subgraph W["worktree's target/"]
        B["ghostty-vt/OTHER, ghosttykit-build/<br/>+ their .zig-cache"]
        G2["ghosttykit/, ghostty-vt/HOST"]
        R2["release/<br/>kanna-* rebuilt"]
        K2["kanna-build/<br/>Swift compiled here"]
    end
    S -.->|"read in place, when clean at the pinned commit"| B
    G -->|"symlink, when ghostty.rev is the pinned commit"| G2
    R -->|"APFS clone (cp -c), kanna-* fingerprints dropped"| R2
    K -->|"APFS clone, workspace-state paths rewritten"| K2
```

| Borrowed | When | How | Saves |
|---|---|---|---|
| Ghostty's source (`kanna_ghostty_source`) | the worktree builds Ghostty itself (another arch, or main's builds are stale) and the main checkout's `ghostty-source` is clean at the pinned commit | used where it is, read-only | cloning the fork |
| Ghostty builds (`kanna_borrow_built`) | the worktree has none and the main checkout's are the pinned commit | symlink | the Zig build |
| cargo's `target/release` (`kanna_borrow_cargo`) | the worktree has no `target/release` | `cp -c -R -p`, then `rm -rf .fingerprint/kanna-*` | the dependencies: cargo 43s → 7s |
| SwiftPM's packages (`kanna_borrow_swiftpm`) | the worktree has no `target/kanna-build` | `cp -c -R -p` of `repositories/` and `checkouts/`; `workspace-state.json` with the main checkout's path replaced by the worktree's | package resolve 3.5s → 0.2s |

- **One Ghostty source builds every target.** `zig build -Dtarget=...` writes wherever `-p` says, so a worktree clones the fork only when it has to. The source is `GHOSTTY_SOURCE_DIR` if set, else the main checkout's source if it's clean at `KANNA_GHOSTTY_COMMIT`, else (in the main checkout, or when main's is missing, stale or edited) its own `target/ghostty-source`.
- **Builds never write into the shared source.** Each build passes `--cache-dir` under its own checkout's `target/` (`ghostty-vt/.zig-cache-TRIPLE`, `ghosttykit-build/.zig-cache`), so two worktrees can build at once. GhosttyKit's build writes `macos/GhosttyKit.xcframework` relative to the directory zig runs in, so `build-ghosttykit.sh` runs zig from `target/ghosttykit-build/` with `--build-file`. Zig fetches packages into the source's `zig-pkg/` only when one is missing, and the main checkout's build has already fetched them.
- **`ghostty.rev` stays right.** `kanna_ghostty_rev` reads the source it built from, and a borrowed source is clean at the pinned commit, so a worktree's builds carry the same rev as main's.
- **Our crates always rebuild.** Dropping their fingerprints makes cargo compile every `kanna-*` crate from the worktree's own source, whatever the main checkout last built; only registry crates, whose fingerprints don't depend on paths, are reused. Binaries and `libkanna_ffi.a` come out byte-identical to a from-scratch build.
- **Built for the same macOS.** The clone carries the main checkout's `macos-deployment-target` stamp; `kanna_cargo_release_for` runs after the borrow and cleans the clone if it was built for another deployment target ([deployment target](deployment-target.md)).
- **Clones, not links.** A clone shares disk blocks until either side writes, so the worktree's builds never write to the main checkout's `target/`, and neither side waits on the other's cargo lock.
- **Only resolution state from SwiftPM.** Its build products record absolute paths (the module cache refuses to load from a different path), so a whole clone of `kanna-build` fails to build; the Swift modules compile in the worktree.

## 2. Dev builds the Swift app in debug

| Channel | Rust (`target/release`) | Swift app |
|---|---|---|
| production, staging | release | release |
| dev (main checkout and tasks) | release | **debug** |

SwiftPM's release build compiles each module whole, on one core: the `Kanna` module (6k lines) takes ~10s whatever changed. Debug compiles files in parallel batches and only what an edit affects. Rust stays release everywhere: its dependencies cost the same either way, the daemon parses every terminal byte, and a dev build's `target/release` is what a worktree borrows. `KANNA_CONFIGURATION=release scripts/build.sh` builds a dev app in release.

## 3. Where `target/` lives

A task worktree's build outputs can live on another volume, so builds stop filling the internal disk: set `build.targets` in your `~/.kanna-v4/config.json` ([settings](settings.md#3-keys)) to a folder there.

```json
{ "build": { "targets": "/Volumes/VHS/kanna-targets" } }
```

```mermaid
flowchart LR
    subgraph I["internal disk"]
        M["~/work/kanna-v4/target/<br/>(Kanna.app, daemon, keeper run here)"]
        W1["…/.kanna-worktrees/task-1dfb12c2/target"]
        W2["…/.kanna-worktrees/task-3f9a1c2e/target"]
    end
    subgraph V["/Volumes/VHS"]
        T1["kanna-targets/kanna-v4/task-1dfb12c2/"]
        T2["kanna-targets/kanna-v4/task-3f9a1c2e/"]
    end
    W1 -->|symlink| T1
    W2 -->|symlink| T2
    M -.->|"borrow: copy across volumes,<br/>APFS clone on the same one"| T1
```

- **Only worktrees move.** A worktree's `target/` becomes a symlink to `ROOT/REPO/WORKTREE` (the main checkout's folder name, then the worktree's). Its sources stay in `.kanna-worktrees`.
- **The main checkout's `target/` stays on the internal disk.** Kanna.app, its daemon and keeper run from it; on a drive that gets unplugged, the keeper and every terminal would die with it.
- **Unset, nothing changes:** `target/` is a folder in the worktree.
- **Set by `kanna_targets`** in `scripts/env.sh`, which `build.sh` and each `build-*.sh` run first. It reads only your file (`$KANNA_AGENTS_HOME/.kanna-v4/config.json` when that's set): the drive belongs to this Mac, not the repo, so a repo's `.kanna/config.json` can't set it.
- **A worktree with a real `target/` moves it over on its next build** (`mv` to `DEST.moving`, rename, link), keeping what it built. A link to another root moves the same way.
- **A missing volume stops the build.** When the root is under `/Volumes/NAME` and `NAME` isn't mounted (absent, or an empty folder left on the internal disk), or `target/` is a link to a folder that isn't there, `build.sh` fails with `build.targets is ROOT, but the volume NAME isn't mounted`, before writing anything. It never falls back to the internal disk.
- **Borrowing copies across volumes.** `cp -c` can't clone across volumes, so `kanna_borrow_cargo` and `kanna_borrow_swiftpm` fall back to `cp -R -p`: ~850 MB over the USB SSD instead of an instant clone. On one volume they still clone. Measured, case (a) below on a USB SSD: **31s** (20.7s on the internal disk); a no-op rebuild 2s.
- **Everything reads it through the link:** cargo, SwiftPM (its `workspace-state.json` paths and module cache keep the worktree's `target/` path), Zig's `--cache-dir`s, `e2e-kanna.sh`, and `--bg` instances launched from the worktree's `target/Kanna.app`. Launch Services runs the app from the volume's real path, so `e2e-kanna.sh` matches either path when it stops an app. `.gitignore` ignores `/target` as well as `target/`, since git doesn't count a symlink as a folder.

## 4. Measured

`scripts/build.sh`, dev channel, in a task worktree, on a 10-core Apple Silicon Mac, the main checkout already built.

| Case | Before | After | Where the time goes after |
|---|---|---|---|
| (a) fresh worktree (`target/` absent) | 71.4s | **20.7s** | cargo, our crates 7.4s · swift debug 11.1s · rest 2s |
| (b) no-op rebuild | 1.2s | **1.0s** | two cargo no-ops 0.2s, `kanna-build` and SwiftPM planning 0.8s |
| (c) edit a Swift file in the `Kanna` module | 9.9s | **5.0s** | 9 files recompiled, then the debug link (~2.5s) |
| (c) edit a Swift leaf module (`KannaStatus`) | 1.4s | **1.2s** | |
| (c) edit `kanna-daemon/src/host.rs` | 3.6s | **3.2s** | cargo 2.3s; Swift untouched |
| (c) edit `kanna-mux` (feeds `kanna-ffi` and the daemon) | 7.9s | **8.0s** | cargo 4.0s, Swift relink 3.0s |

Before, the fresh worktree's 71s was cargo 43s (285 units, 275 CPU-seconds, ~7 of 10 cores), SwiftPM resolve 5s and a release Swift compile of 22s on ~1 core. The merman stack (merman-render, -elk-layered, -core, dugong) is a third of cargo's cold CPU time and its critical path; borrowed dependencies take it off the table.

## 5. Not done

- **sccache:** the borrowed dependencies already cover what it would cache, and it can't share our crates between worktrees (their paths differ).
- **A shared `CARGO_TARGET_DIR`:** concurrent tasks would serialise on cargo's lock and overwrite each other's `kanna-*` builds.
- **A lighter Rust release profile:** our crates' critical path (`kanna-mux` → `kanna-daemon`) is ~7s; the profile is cargo's default (no LTO).
- **Overlapping SwiftPM with cargo:** resolve is 0.2s once borrowed; Swift needs `libkanna_ffi.a` to link.
- **Skipping the bundle copy or the second cargo call:** 0.05s and 0.1s.
