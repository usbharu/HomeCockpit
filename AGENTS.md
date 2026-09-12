# Repository Guidelines

## Project Structure & Module Organization
- `imcp/` contains the core protocol crate plus adapters in `imcp-embedded/`, `imcp-embassy/`, and `imcp-tokio/`. Shared Rust sources live in each crate’s `src/` directory.
- `utils/imcp-cli/` is a Rust CLI utility for inspecting and unpacking IMCP data.
- `firmware/upper_panel_ddi/` is the embedded firmware crate. Keep target-specific code beside `main.rs`, `build.rs`, and `memory.x`.
- `manager/` is the Tauri + Next.js app. UI code lives in `src/app/`, reusable components in `src/components/`, backend code in `src-tauri/`, and static files in `public/`.

## Build, Test, and Development Commands
- `cd imcp && cargo test` runs the protocol and adapter tests.
- `cd utils && cargo test` checks the CLI workspace.
- `cd firmware/upper_panel_ddi && cargo build` compiles the firmware crate.
- `cd manager && npm run dev` starts the web app locally.
- `cd manager && npm run build` produces the production Next.js build.
- `cd manager && npm run tauri dev` launches the desktop app shell during development.

## Coding Style & Naming Conventions
- Use `rustfmt` defaults for Rust code and keep module/file names in `snake_case` with `PascalCase` for types and enums.
- The `imcp` workspace forbids `unwrap()` and `panic!` in normal code paths; prefer explicit error handling and small, testable helpers.
- In `manager/`, TypeScript runs in `strict` mode and uses the `@/` import alias. Keep React components in `PascalCase` and follow the existing kebab-case file naming pattern, such as `device-settings.tsx`.

## Testing Guidelines
- Rust tests use the built-in `#[test]` framework and are colocated with the code they verify.
- Add regression tests near protocol parsing, framing, and adapter logic whenever behavior changes.
- There is no checked-in frontend test runner yet, so validate `manager/` changes with `npm run build` and a manual run through the affected screen.

## Commit & Pull Request Guidelines
- Commit history follows Conventional Commits with scoped subjects, for example `feat(utils): ...` or `fix(firmware): ...`.
- Keep commit messages short, specific, and imperative.
- PRs should explain the user-visible change, list verification commands, and include screenshots or screen recordings for UI changes. Mention firmware or hardware impacts explicitly.

### PR本文の作成と確認
- ユーザーが指定した言語で本文を書く。日本語指定時は、概要・変更内容・検証結果を日本語で記載する。
- `gh pr create`や`gh pr edit`の`--body "...\n..."`は使用しない。ダブルクォート内の`\n`は改行にならず、文字どおり保存される。
- 本文は実際の改行を含むUTF-8ファイルを作成して`--body-file`で渡す。インライン指定が必要な場合は、実改行を含む引数またはシェルのANSI-Cクォート（`$'...\n...'`）を使用する。
- 作成・更新後は`gh pr view <番号> --json body --jq .body`で本文を取得して確認する。`--json body`だけではJSON表現として改行が`\n`にエスケープされるため、`--jq .body`を使って実際の改行、見出し、空行を検証する。
- 本文に`\n`が表示された場合はGitHubの表示不具合と判断せず、`gh pr edit <番号> --body-file <本文ファイル>`で実改行を含む本文に置き換え、再確認する。

## Security & Configuration Tips
- Do not commit secrets, tokens, or device credentials.
- Treat generated binaries and target artifacts as disposable unless a crate explicitly keeps checked-in firmware assets.
