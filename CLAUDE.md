# grabstuff

A Rust command-line tool that queries, slices and compiles local files into agent context. `README.md` describes its use.

- Before a commit: `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`.
- Return `Result` and propagate with `?`; no `unwrap()` or `expect()` outside tests.
- Borrow rather than clone, and reach for `Rc`, `Arc` or `RefCell` only when ownership cannot express the design.
- Wrap any `unsafe` in a safe API.
