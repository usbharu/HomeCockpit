---
name: pico-rust-probe-rs
description: Build, flash, reset, and verify Rust firmware on Raspberry Pi Pico or Pico 2 over SWD with probe-rs.
metadata:
  short-description: probe-rsでPicoへRustを書込み・検証
---

# Raspberry Pi Pico の Rust 書き込み・実機検証

このスキルは、物理 Pico/Pico 2 への Rust 書き込みと、書き込み後の実機動作確認にだけ使う。

## 対象を確定する

crate の `Cargo.toml`、`.cargo/config.toml`、`build.rs`、`memory.x` を確認し、target triple、HAL feature、`probe-rs` の chip 名、ELF パスを一致させる。crate 名だけで判断しない。

| ボード | Rust target の例 | chip 名の例 |
| --- | --- | --- |
| Pico / Pico W (RP2040) | `thumbv6m-none-eabi` | `RP2040` |
| Pico 2 / Pico 2 W (RP2350) | `thumbv8m.main-none-eabihf` | `probe-rs chip list` に出る RP235x 系の名前 |

## SWD 接続と probe の確認

- 対象 Pico とは別の probe-rs 対応 SWD probe を使う。別の Pico を picoprobe にした場合も probe と対象を分ける。
- `SWDIO`、`SWCLK`、`GND` を接続し、対象 Pico は USB または `VSYS` から給電する。SWD 端子から給電しない。
- 複数 probe がある場合は `VID:PID:SERIAL` で対象を明示する。

`probe-rs` が未導入の場合は、公式ツールをインストールする。Windows:

```powershell
irm https://github.com/probe-rs/probe-rs/releases/latest/download/probe-rs-tools-installer.ps1 | iex
```

```powershell
probe-rs --version
probe-rs list
probe-rs chip list
```

## Rust バイナリを作る

target が未導入なら追加し、crate ディレクトリで build する。

```powershell
rustup target add <target-triple>
cargo build --manifest-path <path-to-Cargo.toml>
```

ELF は通常、次の場所にある。

```text
target/<target-triple>/debug/<binary-name>
```

`memory.x` を使う crate では linker 設定と `build.rs` も確認する。

## 接続・書き込み・リセット

```powershell
probe-rs info --probe <probe-selector> --protocol swd --chip <chip-name> --speed 100
```

接続できない場合、対象と probe が reset 線をサポートしていれば `--connect-under-reset` を付けて再試行する。

```powershell
probe-rs info --probe <probe-selector> --protocol swd --chip <chip-name> --speed 100 --connect-under-reset
```

chip 名は `probe-rs chip list` の表示をそのまま使う。書き込みは `download --verify`、続けて `reset`。`download` ではまず `--connect-under-reset` を付けず、失敗時に対象が対応していれば試す。

```powershell
probe-rs download `
  --probe <probe-selector> `
  --chip <chip-name> `
  --protocol swd `
  --speed 100 `
  --verify `
  target\<target-triple>\debug\<binary-name>

probe-rs reset `
  --probe <probe-selector> `
  --chip <chip-name> `
  --protocol swd `
  --speed 100
```

## 実機で検証する

`download --verify` は flash の検証であり、機能の検証ではない。reset 後に、対象 firmware が仕様として持つ観測可能な信号を確認する。LED、GPIO、UART、RTT/defmt などから、対象 crate に実装されているものを選ぶ。ボード固有の LED ピンを成功条件に固定しない。

RTT/defmt を観測する場合は、対象 ELF を `probe-rs run` または crate の runner で起動して期待ログを確認する。`run` は書き込みも行うため、再書き込みしてよい場合に使う。

次を分けて報告する: build、probe/chip 接続、`download --verify`、reset/run、仕様に基づく実機観測。書き込み成功だけで機能成功とはしない。

## 失敗時

- probe が見えない: USB、probe、他プロセスによる占有を確認する。
- target が見えない: 対象電源、`SWDIO`/`SWCLK`/`GND`、chip 名、SWD speed を確認し、対応していれば `info` で `--connect-under-reset` を試す。
- Pico 2 の `Unsupported version (DPv3)`: `probe-rs` を更新する。
- `download` が timeout: `--connect-under-reset` を外し、`--disable-double-buffering` を追加する。
- 観測が失敗: ELF の target/chip と、対象 firmware の仕様に対応する観測手段を確認する。
