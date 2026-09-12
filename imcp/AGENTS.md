# IMCP Agent Guide

このファイルは `imcp/` 以下に適用されます。リポジトリ全体のルールは [`../AGENTS.md`](../AGENTS.md) を参照してください。プロトコル詳細をここへ複製せず、仕様・実装・テストを正とします。

## 先に読むもの

- 不具合全体の切り分け: [`../docs/debugging-scope.md`](../docs/debugging-scope.md)
- ワイヤ仕様、address、ACK/retry、状態遷移: [`../.agents/skills/imcp-protocol/SKILL.md`](../.agents/skills/imcp-protocol/SKILL.md)
- workspace / feature / lint: [`Cargo.toml`](Cargo.toml)
- CI の正確な検証コマンド: [`../.github/workflows/ci.yml`](../.github/workflows/ci.yml)

IMCP の仕様や不具合を扱うときは、必ず protocol skill を読んでから実装を変更してください。意図的に仕様を変える場合は、実装・テスト・skill の記述を同じ変更で同期します。

## 実装の責務

| 対象 | 正本 |
| --- | --- |
| frame model、encode/decode、size limit | [`src/frame.rs`](src/frame.rs) |
| incremental parser、unstuff、再同期 | [`src/parser.rs`](src/parser.rs) |
| client/master state、Join/SetAddress、ACK/retry | [`src/lib.rs`](src/lib.rs) |
| transport 非依存 channel trait | [`src/channel.rs`](src/channel.rs) |
| error taxonomy | [`src/error.rs`](src/error.rs) |
| Embassy adapter | [`imcp-embassy/src/lib.rs`](imcp-embassy/src/lib.rs) |
| embedded I/O / carrier-sense UART | [`imcp-embedded/src/lib.rs`](imcp-embedded/src/lib.rs) |
| Tokio adapter | [`imcp-tokio/src/lib.rs`](imcp-tokio/src/lib.rs) |
| public protocol / state-machine tests | [`tests/core_logic.rs`](tests/core_logic.rs) |
| POSIX PTY wire E2E | [`tests/pty_e2e.rs`](tests/pty_e2e.rs) |

runtime や board 固有の型を core に持ち込まず、adapter 側で channel trait を実装してください。

## 文書化されていない重要事項

- このディレクトリ自体が独立した Cargo workspace です。コマンドは repository root ではなく `imcp/` で実行します。
- core は `no_std` と bounded storage を維持します。protocol path に heap allocation や runtime dependency を追加しないでください。
- decode error、protocol/state error、transport error は呼び出し側が区別できる状態を保ってください。
- parser input は任意の byte 境界で分割されます。壊れた frame の後も、次の正しい SOF から再同期できなければなりません。
- frame の検証が終わる前に protocol state を変更しないでください。重複 SetAddress / ACK のような正当な再送と malformed frame を区別します。
- wire format、checksum、stuffing、payload length、予約 address、frame type、ACK target、retry/state semantics の変更は単なる refactor ではなく protocol migration です。
- `defmt` の有無、および adapter の feature gate を壊さないでください。feature 定義は各 crate の `Cargo.toml` を参照します。

## テストと完了条件

- encoding/decoding は [`src/frame.rs`](src/frame.rs)、streaming/recovery は [`src/parser.rs`](src/parser.rs)、state machine は [`src/lib.rs`](src/lib.rs) または [`tests/core_logic.rs`](tests/core_logic.rs)、実 byte stream は [`tests/pty_e2e.rs`](tests/pty_e2e.rs) に追加します。
- framing、parser、address assignment、ACK/retry、破損入力の修正には、失敗を再現する regression test を必ず追加してください。
- parser error の test では、可能なら直後に valid frame を入力して再同期も確認してください。
- Join/SetAddress の変更では、該当する wrong ID/address/sender、reserved address、duplicate、retry exhaustion、address exhaustion を検討してください。
- PTY test は Unix 固有です。platform gate を維持し、timing-sensitive な E2E だけで unit test を置き換えないでください。
- 最終検証は [CI workflow の IMCP jobs](../.github/workflows/ci.yml) と同じコマンドを `imcp/` で実行します。局所的な `cargo test -p ...` だけで完了にしないでください。

## 下流への影響

公開 API や protocol behavior を変えた場合は、少なくとも以下を検索して影響を確認してください。

- [`../firmware/hcp/`](../firmware/hcp/)
- [`../firmware/homecockpit_firmware_base/`](../firmware/homecockpit_firmware_base/)
- [`../firmware/upper_panel_ddi/`](../firmware/upper_panel_ddi/)
- [`../utils/imcp-cli/`](../utils/imcp-cli/)
- [`../manager/src-tauri/`](../manager/src-tauri/)
