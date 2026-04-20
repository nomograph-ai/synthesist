# Contributing

Thanks for your interest. This crate ships under the nomograph estate and
shares a common Rust contribution flow with `claim`, `workflow`, `synthesist`,
and `lattice`.

## Local checks

```sh
cargo test                                      # run the test suite
cargo fmt                                       # format the tree
cargo clippy --all-targets -- -D warnings       # lint (warnings are errors)
```

CI runs the same four stages (check, fmt, clippy, test) on every push.

## Licensing

All contributions are accepted under the [MIT License](LICENSE). By submitting
a change you agree to license it under those terms.

## Architecture notes

Synthesist is the spec-graph manager. It sits on the claim substrate via the
workflow crate. Before touching the phase machine or store layer, read the
architecture docs in the claim crate:

- `claim/SYNC.md` — sync protocol, heads, and the append/compact boundary
- `claim/IDENTITY.md` — asserter attribution and E2EE
