# Contributing

Thanks for taking a look. sysfun is small on purpose; please keep it that way.

## Ground rules

- Linux only, reading `/proc` and `/sys` directly. Avoid adding crates that
  wrap those; the point is to own the parsers and stay fast.
- No network access from the binary, ever.
- Every view keeps the "plain English first, raw counters last" order.
- Nothing should require root. If a datum needs privileges, show it as
  unavailable with a one-line reason instead.
- Degrade gracefully on machines without the hardware (no battery, no Wi-Fi,
  no sensors).

## Workflow

```sh
cargo fmt
cargo clippy --release
cargo build --release
SYSFUN_TIMING=1 ./target/release/sysfun --once >/dev/null   # check collector cost
for v in overview cpu mem disk net wifi bat temp ps; do
  ./target/release/sysfun --$v --once --width 80 --height 24 >/dev/null || echo "$v failed"
done
```

Please include a `--once` capture (trim anything personal such as SSIDs or
serial numbers) when reporting a layout bug, along with your terminal size
and distribution.

## Changing the logo

Edit `assets/mark.svg`, then run `python3 tools/gen-logo.py` (needs
`rsvg-convert` and ImageMagick) to regenerate `src/ui/logo_data.rs`, and
re-export the PNGs as described in `assets/README.md`.

## Adding a collector or view

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#adding-a-view).
