# kanna

`kanna` drives terminal multiplexers from the command line: kmux (its own
mux), tmux and cmux. See `docs/kanna-spec.md`, and `docs/roadmap.md` for what comes
next for kanna and kmux.

kanna talks to kmux through kmux's control-socket client, vendored from
[kmux](https://github.com/tampopogk/kmux) into `vendor/kmux-client` (MIT).
To update it: `scripts/vendor-kmux-client.sh [REF]`.

```sh
cargo build --release   # target/release/kanna
```

## License

[FSL-1.1-MIT](LICENSE): source-available; each release becomes MIT two years after it is published.
