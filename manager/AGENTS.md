# Manager Agent Guide

このファイルは `manager/` 以下に適用されます。リポジトリ全体のルールは [`../AGENTS.md`](../AGENTS.md) を参照してください。詳細をここへ複製せず、設定・型定義・実装・CI を正とします。

## 先に読むもの

- 不具合全体の切り分け: [`../docs/debugging-scope.md`](../docs/debugging-scope.md)
- package manager / script / dependency: [`package.json`](package.json)、[`pnpm-lock.yaml`](pnpm-lock.yaml)
- Next.js static export: [`next.config.mjs`](next.config.mjs)
- TypeScript 設定と alias: [`tsconfig.json`](tsconfig.json)
- Tauri build / window / bundle 設定: [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json)
- Rust dependency: [`src-tauri/Cargo.toml`](src-tauri/Cargo.toml)
- CI の正確な検証コマンド: [`../.github/workflows/ci.yml`](../.github/workflows/ci.yml)
- DCS-BIOS 接続を扱う場合: [`../.agents/skills/dcs-bios-connection/SKILL.md`](../.agents/skills/dcs-bios-connection/SKILL.md)

## 実装の責務

| 対象 | 正本 |
| --- | --- |
| page composition / global style | [`src/app/`](src/app/) |
| 各画面 | [`src/components/tabs/`](src/components/tabs/) |
| frontend の serialized model | [`src/lib/manager-types.ts`](src/lib/manager-types.ts) |
| Tauri invoke/event と browser fallback | [`src/lib/use-manager-state.ts`](src/lib/use-manager-state.ts) |
| device control metadata | [`src/lib/control-catalog.ts`](src/lib/control-catalog.ts) |
| command、state、persistence、DCS-BIOS、device discovery | [`src-tauri/src/lib.rs`](src-tauri/src/lib.rs) |
| desktop entry point | [`src-tauri/src/main.rs`](src-tauri/src/main.rs) |
| Tauri permission | [`src-tauri/capabilities/`](src-tauri/capabilities/) |

## 文書化されていない重要事項

- frontend は Next.js の static export で、Tauri 2 が `out/` を読み込みます。server-only API を前提にしないでください。
- browser-only の `pnpm run dev` では shell UI は動きますが、hardware、永続化、UDP、serial は Tauri runtime が必要です。`isTauri()` の fallback を維持してください。
- native access は [`use-manager-state.ts`](src/lib/use-manager-state.ts) に集約し、tab component から個別に `invoke()` / `listen()` を増やさないでください。
- Rust struct と [`manager-types.ts`](src/lib/manager-types.ts)、`AppSnapshot` と `defaultSnapshot`、Tauri command 名・argument key・return type、event 名・payload は同じ API 契約です。片側だけ変更しないでください。
- event listener を追加した場合は全ての `UnlistenFn` を cleanup し、重複 listener や dispose 後の更新を防いでください。
- `src-tauri` は [`../dcs-bios-rs`](../dcs-bios-rs)、[`../imcp`](../imcp)、[`../firmware/hcp`](../firmware/hcp) を path dependency として使います。protocol/public type の変更時は各正本と downstream test を確認してください。
- DCS-BIOS の export reception と import command は別経路です。接続診断は repository skill に従い、socket bind/multicast の失敗と単なる無通信を混同しないでください。
- 設定は Tauri app data の `manager-state.json` に保存します。repository や current directory に user settings を書かないでください。
- OS/device I/O 中に shared-state lock を保持し続けないでください。endpoint、port、baud rate、role、mapping、command は保存・利用前に検証します。
- control ID と device kind は firmware/HCP と共有する protocol identifier です。画面上の並び替えを理由に renumber しないでください。
- Tauri capability や CSP を、エラー回避だけを目的に広げないでください。permission 変更は必要性と範囲を明示します。

## Package / generated files

- CI と `packageManager` は pnpm 11.6.0 を正とします。dependency 変更は pnpm を使い、lockfile を手編集しません。
- `package-lock.json` も現在 tracked です。依頼なしに削除・再生成しないでください。
- [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json) の Tauri hook は現状 npm を呼びます。package manager を統一する場合は、この設定・CI・lockfile 方針を一括で変更してください。
- `.next/`、`out/`、`node_modules/`、`target/` は生成物です。`next-env.d.ts` と生成済み Tauri icon も通常は直接編集しません。

## 実装とテスト

- TypeScript strict mode と `@/` alias を維持し、serialized model を component ごとに重複定義しないでください。
- hook、browser global、Tauri API、interactive state を使うファイルは client component の境界を維持します。
- UI 変更では keyboard focus、label、disabled/busy、empty/error/browser-only state を確認してください。
- Tauri command は薄く保ち、validation・変換・protocol 処理を unit test 可能な helper に分けます。
- backend test は [`src-tauri/src/lib.rs`](src-tauri/src/lib.rs) の既存 test module を基準に追加し、real DCS、serial device、固定 port、個人の home directory、race-dependent sleep に依存させないでください。
- frontend test runner は現在ありません。UI は production build と対象画面の manual smoke test、native behavior は Tauri runtime で確認します。
- 最終検証は [CI workflow の Manager frontend/backend jobs](../.github/workflows/ci.yml) と同じコマンドを各 working directory で実行します。Linux では Tauri/serialport の native package が必要です。
- UI の PR には screenshot または screen recording を添えます。network/serial/hardware の検証は環境と観測結果を記載し、実機未確認なら明記してください。
