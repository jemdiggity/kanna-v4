# kanna

`kanna` drives terminal multiplexers from the command line: kmux (its own
mux), tmux and cmux. See `docs/kanna-spec.md`, and `docs/roadmap.md` for what comes
next for kanna and kmux.

kanna uses the kmux control-socket client from the
[kmux](../kmux) repo, which must be checked out next to this one:

```
work/
  kanna-v4/   this repo
  kmux/       the kmux app, CLI, spec and kmux-client crate
```

```sh
cargo build --release   # target/release/kanna
```
