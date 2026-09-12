# HomeCockpit Agent Guide

このファイルはリポジトリ全体の案内です。詳細をここへ複製せず、原則として実装・設定・専用ドキュメントを正とします。サブディレクトリに `AGENTS.md` がある場合は、そちらも併せて参照してください。

## 最初に確認するもの

- 不具合調査の範囲・必要環境・ユーザー操作: [`docs/debugging-scope.md`](docs/debugging-scope.md)
- CI と検証コマンド: [`.github/workflows/ci.yml`](.github/workflows/ci.yml)
- Firmware のbuild・書き込み・実機検証: [`firmware/AGENTS.md`](firmware/AGENTS.md)
- IMCP の作業規約: [`imcp/AGENTS.md`](imcp/AGENTS.md)
- Manager の作業規約: [`manager/AGENTS.md`](manager/AGENTS.md)
- IMCP ワイヤ仕様: [`.agents/skills/imcp-protocol/SKILL.md`](.agents/skills/imcp-protocol/SKILL.md)
- DCS-BIOS 接続診断: [`.agents/skills/dcs-bios-connection/SKILL.md`](.agents/skills/dcs-bios-connection/SKILL.md)
- Pico/Pico 2 の SWD 手順: [`.agents/skills/pico-rust-probe-rs/SKILL.md`](.agents/skills/pico-rust-probe-rs/SKILL.md)

作業内容が上記スキルの対象なら、該当する `SKILL.md` を先に読んでください。
デバッグや原因調査では、まず [`docs/debugging-scope.md`](docs/debugging-scope.md) で必要な component と最初の観測点を決めてください。

## 構成と正本

| 対象 | 正本・入口 |
| --- | --- |
| IMCP core / adapter | [`imcp/Cargo.toml`](imcp/Cargo.toml)、[`imcp/src/`](imcp/src/)、[`imcp/tests/`](imcp/tests/) |
| Firmware全体 | [`firmware/AGENTS.md`](firmware/AGENTS.md) |
| HCP application protocol | [`firmware/hcp/README.md`](firmware/hcp/README.md)、[`firmware/hcp/src/lib.rs`](firmware/hcp/src/lib.rs) |
| firmware 共通処理 | [`firmware/homecockpit_firmware_base/README.md`](firmware/homecockpit_firmware_base/README.md) |
| Upper Panel DDI firmware | [`firmware/upper_panel_ddi/README.md`](firmware/upper_panel_ddi/README.md)、[`firmware/upper_panel_ddi/.cargo/config.toml`](firmware/upper_panel_ddi/.cargo/config.toml) |
| Pico 2 bring-up | [`firmware/pico2_blinky/README.md`](firmware/pico2_blinky/README.md) |
| DCS-BIOS library | [`dcs-bios-rs/src/lib.rs`](dcs-bios-rs/src/lib.rs)、[`dcs-bios-rs/src/import.rs`](dcs-bios-rs/src/import.rs) |
| IMCP CLI | [`utils/imcp-cli/src/main.rs`](utils/imcp-cli/src/main.rs) |
| Manager | [`manager/AGENTS.md`](manager/AGENTS.md) |
| KiCad hardware | [`upper_panel_ddi/`](upper_panel_ddi/)、[`upper_panel_ddi_button_panel/`](upper_panel_ddi_button_panel/)、共有ライブラリ [`HomeCockpit.kicad_sym`](HomeCockpit.kicad_sym) / [`Library.pretty/`](Library.pretty/) |

## 文書化されていない重要事項

- ルートは Cargo workspace ではありません。各 Cargo project/workspace のディレクトリでコマンドを実行してください。対象と CI コマンドの対応は [CI workflow](.github/workflows/ci.yml) が正です。
- `firmware/lib/embassy` と `firmware/lib/trouble` は [git submodule](.gitmodules) です。明示的な依頼なしに中身や gitlink を更新しないでください。
- IMCP は framing/address/ACK・再送を、HCP は IMCP payload 上の application message を担当します。HCP は 1 packet が IMCP の最大 payload 128 bytes に収まる前提です。
- `manager/src-tauri` は `dcs-bios-rs`、`imcp`、`firmware/hcp` を path dependency として利用します。公開型やプロトコルの変更時は Manager への影響も確認してください。
- `target/`、`manager/.next/`、`manager/out/`、KiCad の `production/` やバックアップは生成物です。実装として編集・コミットしないでください。
- `firmware/lib/cyw43-firmware/*.bin` はライセンス付きの upstream firmware です。通常の生成物として変更しないでください。
- ハードウェアを使っていない場合、ビルド成功を実機検証済みとして報告しないでください。実機確認時は board、probe、transport、観測結果を記録してください。

## 変更時の原則

- 既存の Rust lint は各 `Cargo.toml` を正とし、`rustfmt` と Clippy に従ってください。特に protocol/shared firmware の通常経路では `unwrap()` / `panic!` を避けます。
- wire value、永続化 JSON、Tauri command/event、device/control ID の変更は互換性変更として扱い、producer と consumer と regression test を同時に更新してください。
- dependency 変更以外で lockfile を更新しないでください。lockfile は手編集しません。
- protocol、セットアップ、hardware procedure を変えた場合は、コードだけでなく上記の正本も更新してください。
- secrets、token、device credential、署名情報、個人環境の絶対パスや固定 probe ID をコミットしないでください。
- 完了前に変更対象に対応する [CI の format / test / Clippy / build](.github/workflows/ci.yml) を実行してください。環境や実機の都合で実行できない検証は、未実施理由を明記してください。

## Commit / PR

- 既存履歴に合わせて Conventional Commits と適切な scope（例: `fix(imcp):`、`feat(manager):`、`chore(ci):`）を使います。
- PR には変更理由、互換性・hardware への影響、実行した検証を記載します。UI は screenshot、基板は ERC/DRC、firmware は可能なら実機結果を添えてください。
