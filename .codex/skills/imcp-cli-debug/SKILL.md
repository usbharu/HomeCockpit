---
name: imcp-cli-debug
description: Debug IMCP serial traffic with imcp-cli by generating, decoding, simulating, and sending protocol frames as an LLM-readable JSON stream.
metadata:
  short-description: imcp-cliでIMCP通信を再現・観測・送信
---

# IMCP-CLIでプロトコルをデバッグする

IMCPのフレーム、JOIN割り当て、ACK、PING、SET、チェックサム破損、再送を、実機またはstdinシミュレーションで切り分けるときに使う。対象は`utils/imcp-cli`であり、ファームウェアの書き込みや変更はこのスキルの範囲に含めない。

## 最初に確認する

- リポジトリの`AGENTS.md`と`utils/imcp-cli`の`Cargo.toml`を読む。
- CLIが未ビルドなら`cargo build --manifest-path utils/Cargo.toml`を実行する。ビルド時にsccacheが失敗する環境では、PowerShellで`$env:RUSTC_WRAPPER=''`を設定して再実行する。
- 実機のポートを推測せず、`imcp-cli watch --list --format json`で確認する。`COM3`などを固定値としてスキルの手順に埋め込まない。
- 送信は機器状態を変える可能性がある。実機への`master --port`または`--control-stdin`による送信は、ユーザーが実機テストを依頼した場合だけ行い、まず`pack`と`unpack`でフレームを確認する。

## 標準ワークフロー

1. `pack`で送信・入力用のフレームを作る。アドレスは通常、masterが`0x01`、未割り当てclientが`0x00`、broadcastが`0xFF`、最初の割り当て先が`0x02`である。
2. `unpack --format json --data <HEX>`で、方向、アドレス、フレーム種別、payloadを確認する。
3. 実機に触れずに試す場合は`master --stdin --format json`へ、ワイヤ上の16進数を1行ずつ渡す。起動時のmaster発信は`--send <HEX>`を繰り返し指定する。
4. 実機を観測する場合は`master --port <PORT> --format json --control-stdin`を使い、標準入力から`send <HEX>`または`<HEX>`を送る。受信・応答・エラーをJSON Linesとして記録する。
5. 「応答がない」ときは、まず宛先と送信元を確認する。masterの自動応答はmaster宛てのJOIN、PING、SETに対して発生し、master宛てでないPINGやDATAには通常応答しない。

詳細な再現シナリオと期待結果は[references/scenarios.md](references/scenarios.md)を読む。

## 観測結果の読み方

- `event=frame`はパーサーが受け取ったフレーム、`direction=rx/tx`はCLIから見た方向を表す。
- `event=bytes`は送信したワイヤ上の16進数である。masterの応答は、フレームイベントではなくこのイベントで出ることがある。
- `event=error`は入力hex、チェックサム、プロトコル処理などのエラーである。JSONモードの標準出力をそのまま機械処理し、標準エラーだけを成功判定に使わない。
- JOINに対するSetAddressは宛先`0x00`、ACK payloadは`0x00`になる。ACKを失うとserialとstdinの両モードでSetAddressを一定間隔で再送し、上限到達後に保留状態を破棄する。
- チェックサム破損後に後続フレームを送って、`error`の後に後続フレームが正常に観測できるかを確認する。これでパーサーの再同期を検証できる。

## 修正を加えた場合の検証

CLIの変更後は、少なくとも次を実行する。

```powershell
cargo fmt --manifest-path utils/Cargo.toml -- --check
cargo test --manifest-path utils/Cargo.toml
cargo clippy --manifest-path utils/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path imcp/Cargo.toml --features test-utils
```

失敗時は、フレーム生成、パーサー、master状態機械、シリアルI/Oのどの層で失敗したかを分けて報告する。実機未接続なら、stdinシミュレーションで確認できた範囲と実機未確認の範囲を明記する。
