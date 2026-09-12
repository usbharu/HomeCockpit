# Firmware Agent Guide

このファイルは `firmware/` 以下に適用されます。リポジトリ全体のルールは [`../AGENTS.md`](../AGENTS.md) を参照してください。build、flash、reset、実機検証の詳細手順をここへ複製せず、各crateの設定・README・専用skillを正とします。

## 先に読むもの

- 不具合調査のscopeと必要な実機操作: [`../docs/debugging-scope.md`](../docs/debugging-scope.md)
- Pico / Pico 2 のbuild・SWD・flash・reset・実機検証: [`../.agents/skills/pico-rust-probe-rs/SKILL.md`](../.agents/skills/pico-rust-probe-rs/SKILL.md)
- CIのbuild targetと検証コマンド: [`../.github/workflows/ci.yml`](../.github/workflows/ci.yml)
- submoduleの定義: [`../.gitmodules`](../.gitmodules)

実機への接続、書き込み、reset、RTT/defmt確認を行う場合は、必ず `pico-rust-probe-rs` skillを先に読んでください。

## Crateと正本

| 対象 | 用途 | board・target・手順の正本 |
| --- | --- | --- |
| [`hcp/`](hcp/) | HCP application protocol。board非依存、`no_std` | [`hcp/README.md`](hcp/README.md)、[`hcp/Cargo.toml`](hcp/Cargo.toml) |
| [`homecockpit_firmware_base/`](homecockpit_firmware_base/) | 共通runtime helper。board固有GPIOを置かない | [`homecockpit_firmware_base/README.md`](homecockpit_firmware_base/README.md)、[`homecockpit_firmware_base/Cargo.toml`](homecockpit_firmware_base/Cargo.toml) |
| [`upper_panel_ddi/`](upper_panel_ddi/) | Upper Panel DDIの実機firmware | [`upper_panel_ddi/README.md`](upper_panel_ddi/README.md)、[`upper_panel_ddi/Cargo.toml`](upper_panel_ddi/Cargo.toml)、[`upper_panel_ddi/.cargo/config.toml`](upper_panel_ddi/.cargo/config.toml)、[`upper_panel_ddi/memory.x`](upper_panel_ddi/memory.x) |
| [`pico2_blinky/`](pico2_blinky/) | Pico 2 bring-up用の独立firmware | [`pico2_blinky/README.md`](pico2_blinky/README.md)、[`pico2_blinky/Cargo.toml`](pico2_blinky/Cargo.toml)、[`pico2_blinky/.cargo/config.toml`](pico2_blinky/.cargo/config.toml)、[`pico2_blinky/memory.x`](pico2_blinky/memory.x) |
| [`lib/`](lib/) | Embassy / Trouble submoduleとCYW43 firmware blob | [`../.gitmodules`](../.gitmodules)、[`lib/cyw43-firmware/README.md`](lib/cyw43-firmware/README.md) |

`test/` はfirmware配下ではなくrepository rootの独立したhardware experimentです。対象をcrate名だけで推測せず、[`../test/Cargo.toml`](../test/Cargo.toml) と [`../test/.cargo/config.toml`](../test/.cargo/config.toml) を別途確認してください。

## 実機書き込み前に確定する情報

以下が揃うまで、buildやread-onlyのprobe確認は進めても、firmwareを書き込まないでください。

- 対象crateとbinary名
- 実機のboard名、およびMCUがRP2040かRP235xか
- `Cargo.toml`のHAL feature、`.cargo/config.toml`のRust targetとrunner、`memory.x`、`build.rs`が同じMCUを指していること
- debug/releaseのどちらを書き込むかと、実際に書き込むELFの絶対path
- 使用するdebug probe、接続protocol、複数probeがある場合の一意なselector
- SWDIO、SWCLK、GND、必要ならRESET、および対象boardの給電方法
- 既存firmwareを上書きしてよいこと。保持すべき設定や回収すべきbinaryの有無
- 書き込み後に使うtransport（USB CDC / UART / Hub経由）と必要な配線
- reset後の成功条件と観測手段（RTT/defmt、USB認識、UART frame、GPIO、LED、button eventなど）
- ユーザーに必要な操作と、その操作を依頼する時点

エージェントは書き込み前に、少なくとも次をユーザーへ明示してください。

```text
対象: <board / MCU / crate / binary>
書き込むもの: <profile / target triple / ELF path>
接続: <probe selector / SWD / power>
上書きされるもの: <現在のfirmware>
書き込み後の確認: <reset方法 / 観測点 / 成功条件>
ユーザーに必要な操作: <USB接続、button操作など>
```

## 安全性と操作境界

- board名とMCU、target triple、probe-rsのchip名が一致しない状態でflashしないでください。値はREADMEの例ではなく、対象crateの設定と接続した実機から確定します。
- probeと書き込み対象のPicoを区別してください。SWD端子から対象boardへ給電せず、GNDを共有します。
- `probe-rs list`、`probe-rs chip list`、`probe-rs info`は書き込み前の確認に使えます。`probe-rs run`は書き込みを伴うため、read-only確認として扱いません。
- 原則として検証付きdownloadを使い、その後にresetします。正確なoptionと失敗時の切り分けは [`pico-rust-probe-rs/SKILL.md`](../.agents/skills/pico-rust-probe-rs/SKILL.md) に従ってください。
- `download --verify`の成功はflash内容の一致だけを示します。firmwareの機能成功は、reset後の仕様に対応した出力を別途観測して判断します。
- USB抜き差し、電源再投入、button操作、配線確認は、判定したい境界・操作順・期待結果・採取するlogを示してユーザーへ依頼してください。
- 配線や部品を変更するときは先に電源を切ります。software/transportの観測で回路を除外できていない段階で、回路変更を要求しないでください。
- probe selector、COM/serial port、絶対pathなど個人環境の値をsourceや設定へcommitしないでください。

## Buildと変更時の注意

- 各firmware crateは独立しており、repository rootはCargo workspaceではありません。working directoryとCI commandは [`../.github/workflows/ci.yml`](../.github/workflows/ci.yml) を正とします。
- `upper_panel_ddi`の依存先にはgit submoduleがあります。欠けている場合は内容を代替実装せず、[`../.gitmodules`](../.gitmodules) に従って初期化状況を確認してください。
- HCP/IMCPのwire value、payload、address、ACK/retryを変更する場合は、[`../.agents/skills/imcp-protocol/SKILL.md`](../.agents/skills/imcp-protocol/SKILL.md) と下流componentを確認してください。
- board固有のpin、matrix scan、USB/UART setupは実機firmwareに置き、[`homecockpit_firmware_base/`](homecockpit_firmware_base/)へ持ち込まないでください。
- hardware依存コードではheap、stack、blocking、割り込み、電力、timingへの影響を確認します。既存lintは各crateの`Cargo.toml`を正とします。
- `target/`は生成物です。CYW43 `.bin`とsubmoduleは通常のbuild artifactではないため、明示的な目的なしに変更しません。

## 報告する結果

次を別々の結果として報告してください。途中の成功から後段の成功を推測しないでください。

1. sourceと設定から確定したboard / target / chip / binary
2. build結果
3. probe列挙とtarget接続結果
4. flashとverify結果
5. resetまたはrun結果
6. RTT/defmt、USB、UART、GPIO、buttonなどの機能観測
7. 未確認事項、必要なユーザー操作、回路検証の要否
