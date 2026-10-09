# Kanna

`kanna` drives a mux from the command line: Kanna.app, kmux, and later tmux and cmux. See [docs/kanna-spec.md](docs/kanna-spec.md), and [docs/roadmap.md](docs/roadmap.md) for what comes next. Its default mux is **Kanna.app**, Kanna's own build of [kmux](https://github.com/tampopogk/kmux), made from a kmux checkout beside this repo (`../kmux`).

```sh
git clone https://github.com/tampopogk/kmux.git ../kmux   # once
scripts/build-kanna.sh          # ../kmux/target/Kanna.app and target/release/kanna
kanna open md docs/kanna-spec.md
kanna open term --name server --cmd "npm run dev" --split right
kanna list
```

`kanna` starts Kanna.app if it isn't running. `--mux kmux` drives a kmux you run instead.

`kmux.rev` records the kmux commit kanna was last built and tested with; the build warns when `../kmux` is elsewhere. Changes Kanna needs from kmux go into kmux itself (e.g. its brand setting, which gives Kanna.app its own name, icon and sockets). Set `KMUX_DIR` to build from another checkout.

The first kmux build compiles GhosttyKit with Zig, which takes a while; Kanna.app reuses whatever `../kmux` has built.

## License

[FSL-1.1-MIT](LICENSE): source-available; each release becomes MIT two years after it is published. kmux and Ghostty are MIT; [NOTICE](NOTICE) lists the code derived from them.
