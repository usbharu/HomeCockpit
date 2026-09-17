# upper_panel_ddi firmware

`upper_panel_ddi` は Raspberry Pi Pico（RP2040）向けのファームウェアです。
Cargo のデバッグ runner には `probe-rs run` を使用します。

## 必要なツール

- Rust toolchain
- `thumbv6m-none-eabi` ターゲット
- `probe-rs`（`probe-rs-tools`）
- RP2040 に接続した SWD 対応の Debug Probe

`probe-rs` は [公式インストール手順](https://probe.rs/docs/getting-started/installation/)
に従ってインストールし、`probe-rs` コマンドが `PATH` に含まれていることを確認してください。

```powershell
rustup target add thumbv6m-none-eabi
probe-rs --version
```

## Probe とターゲットの確認

```powershell
probe-rs list
probe-rs info --chip RP2040 --protocol swd
```

`probe-rs list` に Debug Probe が表示され、`probe-rs info` が RP2040 に接続できれば、
書き込みの準備は完了です。複数の Probe を接続している場合は、必要に応じて
`--probe VID:PID[:SERIAL]` をコマンドへ追加してください。runner の設定には特定の
Probe ID を固定していません。

## ビルドと実行

```powershell
cd C:\Users\haruj\Documents\HomeCockpit\firmware\upper_panel_ddi
cargo build --locked --bin upper_panel_ddi
cargo run --locked --bin upper_panel_ddi
```

`.cargo/config.toml` の runner は次の設定です。

```toml
runner = "probe-rs run --chip RP2040"
```

`cargo run` は `probe-rs run` によってファームウェアを書き込み、ターゲットを
リセットして実行します。`defmt-rtt` の RTT/defmt ログは同じコンソールへ出力されます。
ログレベルは `.cargo/config.toml` の `DEFMT_LOG = "debug"` で設定されています。

## panic と HardFault の診断

`panic-probe` と `defmt-rtt` をリンクしているため、panic 発生時の情報は
`probe-rs run` のコンソールで確認できます。HardFault の診断時は、ビルド済み ELF を
直接実行してスタックトレースを常に表示できます。

```powershell
probe-rs run --chip RP2040 --always-print-stacktrace `
  target\thumbv6m-none-eabi\debug\upper_panel_ddi
```

通常の開発では `cargo run --locked --bin upper_panel_ddi` を使用してください。

## IMCP 通信の確認

この crate は USB CDC 接続中は USB CDC（Windows では COM ポート）を、USB CDC が
接続されていない場合は UART を IMCP transport として使用します。どちらも
115200 baud の IMCP 通信です。ファームウェア実行後、既存の IMCP ホストを接続して、
次の動作を確認します。

1. デバイスから JOIN が送信される。
2. アドレス割り当てを受信する。
3. ACK が返る。
4. ボタン操作で制御イベントが送信される。
5. USB CDC または UART 経由の通信が継続する。

Manager などのホストから HCP の `RequestDeviceHello` を受信した場合、デバイスは
現在の IMCP address から `DeviceHello` を再送します。これにより、デバイスが既に
`Ready` 状態で起動した Manager に接続された場合も再発見できます。
送信キューには ACK と `DeviceHello` 用の 2 枠を予約しており、通常のボタンイベントが
この予約枠を消費しないようにしています。Manager 側も取りこぼしに備え、100ms 間隔で
最大 4 回（初回を含む）`RequestDeviceHello` を送信します。

USB CDC 実装の追加や、全 RAM を対象とした stack watermark の導入は、この runner 移行の
範囲には含めません。

## `probe-run` からの移行

以前の runner は `probe-run --chip RP2040` でした。`probe-run` はメンテナンスモードの
ため、現在は後継の `probe-rs run --chip RP2040` を使用します。runner では Probe ID、
接続速度、`--connect-under-reset` を固定せず、接続環境に応じた指定をコマンドラインから
追加できるようにしています。
