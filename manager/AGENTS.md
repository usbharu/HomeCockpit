# Repository Guidelines

## Project Structure & Module Organization
- `src/app/` contains the Next.js app router entry points (`layout.tsx`, `page.tsx`, `globals.css`).
- `src/components/` holds reusable UI; tab-specific screens live in `src/components/tabs/`.
- `src-tauri/` contains the Rust backend for the Tauri desktop shell, including `src/main.rs`, `build.rs`, `Cargo.toml`, and `tauri.conf.json`.
- `public/` stores static assets used by the web app and Tauri frontend.

## Build, Test, and Development Commands
- `npm run dev` starts the Next.js app with Turbopack.
- `npm run build` creates the production web build and is the main validation step for UI changes.
- `npm run start` runs the built web app locally.
- `npm run tauri dev` launches the desktop app shell during development.
- `cd src-tauri && cargo build` checks the Rust backend compiles.

## Coding Style & Naming Conventions
- TypeScript runs in `strict` mode; keep types explicit where inference is weak.
- Use the `@/` import alias for `src/` imports.
- Name React components in `PascalCase` and prefer kebab-case filenames, such as `device-settings.tsx`.
- Follow the existing formatting in each file; keep JSX and Rust code aligned with `rustfmt` and the current project style.

## Testing Guidelines
- There is no checked-in frontend test runner yet.
- Validate UI and app-flow changes with `npm run build` and a manual smoke test in the affected screen.
- For Rust behavior changes, add colocated `#[test]` coverage in `src-tauri/src/` when practical and verify with `cargo test`.

## Commit & Pull Request Guidelines
- Commit history uses Conventional Commits with scopes, for example `feat(manager): ...` or `fix(firmware): ...`.
- Keep messages short, specific, and imperative.
- PRs should summarize the user-visible change, list verification commands, and include screenshots or screen recordings for UI work.
- Call out any Tauri, Rust, or device-hardware impact explicitly.

## Security & Configuration Tips
- Do not commit secrets, API tokens, or device credentials.
- Treat generated build artifacts as disposable unless a crate explicitly keeps them under version control.
