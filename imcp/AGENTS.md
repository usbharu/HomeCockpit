# Repository Guidelines

## Project Structure & Module Organization
- `src/` contains the core `imcp` protocol crate. Main modules include `frame.rs`, `parser.rs`, `channel.rs`, and `error.rs`.
- `imcp-embedded/`, `imcp-embassy/`, and `imcp-tokio/` are workspace members that adapt the protocol for embedded and async runtimes.
- Keep target-specific firmware code beside its crate entry points such as `main.rs`, `build.rs`, and `memory.x` when those crates are present.
- Tests live next to the code they verify, usually in `mod tests` blocks inside the same file.

## Build, Test, and Development Commands
- `cargo test` from the repository root runs the Rust workspace tests.
- `cargo test -p imcp-embassy` runs tests for a single crate when you only change one adapter.
- `cargo build -p imcp-tokio` verifies an individual crate compiles.
- Use `cargo fmt` before submitting Rust changes.

## Coding Style & Naming Conventions
- Follow `rustfmt` defaults and keep module and file names in `snake_case`.
- Types, enums, and traits should use `PascalCase`; functions, variables, and fields should use `snake_case`.
- This workspace forbids `unwrap()` and `panic!` in normal code paths. Prefer explicit error handling and small helpers.
- Respect the existing Rust lint settings in `Cargo.toml`, especially the Clippy rules around `unwrap`, `expect`, and `panic`.

## Testing Guidelines
- Use the built-in Rust `#[test]` framework.
- Add regression tests near protocol parsing, framing, encoding, and adapter behavior when changing behavior.
- Prefer focused unit tests that exercise a single protocol rule or edge case.

## Commit & Pull Request Guidelines
- Commit messages follow Conventional Commits with scopes, for example `feat(imcp): ...` or `fix(firmware): ...`.
- Keep subjects short, imperative, and specific.
- PRs should explain the user-visible change, list verification commands, and mention any firmware or hardware impact.

## Security & Configuration Tips
- Do not commit secrets, tokens, or device credentials.
- Treat generated binaries and build artifacts as disposable unless a crate explicitly checks them in.
