# デバッグ範囲・必要環境の切り分け

この文書は、不具合調査を始める前に必要な component、実機、ユーザー操作、観測点を決めるための正本です。個別の protocol や書き込み手順は、末尾のリンク先を参照してください。

## 最初に確認する情報

調査開始時に、分かる範囲で次を記録します。不明な項目を推測で埋めず、最初の観測で確定してください。

- 期待した動作と、実際の動作
- 最後に正常だった地点または確認済みの地点
- DCS World と Manager が同一 PC か別 PC か
- 使用中の DCS profile、aircraft/module、mission が開始済みか
- DCS-BIOS の導入有無と export/command endpoint
- Manager の起動方法、表示状態、エラー、該当ログ
- 接続する device の種類、Pico/Pico 2、firmware、接続方式（USB CDC / UART / Hub 経由）
- 対象 serial port、baud rate、Manager の endpoint role
- debug probe の種類と SWD 接続有無
- 実施できる物理操作（電源再投入、USB 差し直し、button 操作、配線確認）
- firmware 書き込みや DCS cockpit state の変更を行ってよいか

調査を始めるエージェントは、作業前または最初の観測後に次の形で範囲を明示します。

```text
対象経路:
必要: DCS / DCS-BIOS / Manager / HCP / IMCP / Hub / Pico / 回路 のうち該当するもの
現時点で不要:
まず行うread-only確認:
ユーザーに必要な操作:
状態を変更する操作（ある場合）:
この段階の成功条件:
```

初期情報だけで判断できない項目は「必要」と決め打ちせず、「条件付き」として、必要性を確定するための最小の観測を先に示してください。

## システム境界

DCS 側と device 側は Manager で合流します。最初から end-to-end 全体を動かさず、矢印ごとに入力と出力を観測してください。

```text
DCS World
  -> Export.lua / DCS-BIOS
  -> UDP export
  -> Manager listener / dcs-bios-rs
  -> Manager state and UI

Button / switch
  -> Pico GPIO or matrix scan
  -> HCP ControlEvent
  -> IMCP frame and address state
  -> USB CDC or UART
  -> [optional: IMCP Hub]
  -> Manager device endpoint / mapping
  -> DCS-BIOS import command
  -> DCS World cockpit state
```

ある境界の入力が確認でき、出力が確認できなければ、その境界を担当する component を優先して調べます。上流の入力が未確認なら、下流のコード変更を始めないでください。

## 目的別の必要環境

記号: `必須` = その確認に必要、`不要` = 切り離して確認可能、`条件` = 対象経路による。

| 調査目的 | DCS | DCS-BIOS | Manager | HCP | IMCP | Hub | Pico | 書き込み | 回路/操作 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Rust/TypeScript の compile・unit test | 不要 | 不要 | 条件 | 条件 | 条件 | 不要 | 不要 | 不要 | 不要 |
| DCS-BIOS export が PC に届くか | 必須 | 必須 | 不要 | 不要 | 不要 | 不要 | 不要 | 不要 | mission開始・値変更が条件 |
| Manager が DCS-BIOS を受信・表示するか | 必須 | 必須 | 必須 | 不要 | 不要 | 不要 | 不要 | 不要 | mission開始・値変更が条件 |
| DCS-BIOS import command が効くか | 必須 | 必須 | 条件 | 不要 | 不要 | 不要 | 不要 | 不要 | cockpit変化の確認が必須 |
| HCP encode/decode の検証 | 不要 | 不要 | 不要 | 必須 | 不要 | 不要 | 不要 | 不要 | 不要 |
| IMCP frame/parser/state machine の検証 | 不要 | 不要 | 不要 | 不要 | 必須 | 不要 | 不要 | 不要 | 不要 |
| Manager の direct device discovery | 不要 | 不要 | 必須 | 必須 | 必須 | 不要 | 必須 | firmware次第 | USB/UART接続が必須 |
| Hub と child device の列挙 | 不要 | 不要 | 必須 | 必須 | 必須 | 必須 | 必須 | firmware次第 | Hub/child接続が必須 |
| button event が Manager に届くか | 不要 | 不要 | 必須 | 必須 | 必須 | 条件 | 必須 | firmware次第 | button操作が必須 |
| button から DCS までの end-to-end | 必須 | 必須 | 必須 | 必須 | 必須 | 条件 | 必須 | firmware次第 | mission・button操作が必須 |
| Pico firmware の build | 不要 | 不要 | 不要 | 条件 | 条件 | 条件 | 不要 | 不要 | 対象board/targetの情報が必須 |
| Pico と debug probe の接続確認 | 不要 | 不要 | 不要 | 不要 | 不要 | 不要 | 必須 | 不要 | probe・SWD・給電が必須 |
| Pico への firmware flash | 不要 | 不要 | 不要 | 条件 | 条件 | 条件 | 必須 | 必須 | probe・SWD・給電が必須 |
| Pico 上の firmware 動作・RTT確認 | 不要 | 不要 | 不要 | 条件 | 条件 | 条件 | 必須 | firmware次第 | reset、log、仕様上の出力確認が必須 |
| KiCad schematic / PCB の静的検証 | 不要 | 不要 | 不要 | 不要 | 不要 | 条件 | 不要 | 不要 | ERC/DRC、設計確認 |
| GPIO、matrix、UART の実機電気検証 | 不要 | 不要 | 不要 | 不要 | 条件 | 条件 | 必須 | 条件 | 導通・電圧・信号・操作確認が必須 |
| 配線・部品・回路の変更 | 不要 | 不要 | 不要 | 不要 | 条件 | 条件 | 条件 | 条件 | 原因箇所の特定と電源OFFが必須 |

`Manager = 条件` の import command test は、Manager 統合を調べる場合だけ必要です。DCS-BIOS 単体の command path は別の送信手段でも確認できます。`firmware次第` は、対象 Pico に調査対象の build が既に入っていると証明できれば再書き込み不要、証明できなければ書き込みが必要という意味です。probe の列挙や `probe-rs info` だけなら firmware を上書きする必要はありません。`probe-rs run` は書き込みを伴うため、read-only の接続確認とは区別してください。

現状のリポジトリには HCP の `DeviceKind::ImcpHub` と Manager の Hub/child 列挙処理はありますが、専用の IMCP Hub firmware crate はありません。Hub 経路の実機検証を計画するときは、使用する Hub hardware、firmware の所在・version、接続 topology をユーザーに確認してください。

## 症状から最初の観測点を選ぶ

| 症状 | 最初に確認する証拠 | 主な範囲 | 最初は不要なもの |
| --- | --- | --- | --- |
| Manager が `listening` のまま | read-only detector の datagram/record 数、DCS-BIOS endpoint | DCS / Export.lua / network | Pico、HCP、IMCP、Hub、回路 |
| Manager が `error` | exact error、UDP port 使用状況、bind/multicast join | Manager / OS network | Pico、HCP、IMCP |
| packet 数は増えるが値が更新されない | valid logical records、address/value、datagramをまたぐframe | dcs-bios-rs / Manager decode | Pico、Hub |
| Manager からの command が効かない | 宛先、control reference、送信内容、DCS上の変化 | DCS-BIOS command path | HCP、IMCP、Pico |
| serial port が一覧に出ない | OS の port 列挙、USB認識、他processの占有 | USB / driver / Manager | DCS、DCS-BIOS |
| port はあるが device を発見できない | raw bytes、Join、SetAddress、ACK、DeviceHello | firmware / HCP / IMCP / Manager | DCS、DCS-BIOS |
| direct device は見えるが child が見えない | root の DeviceKind、role hint、RequestDeviceHello、child frame | Hub / child / Manager enumeration | DCS、DCS-BIOS |
| button を押しても Manager に来ない | GPIO/matrix log、HCP ControlEvent、IMCP frame、Manager log | Pico / HCP / IMCP / transport | DCS（mapping前まで） |
| Manager には button が来るが DCS が変わらない | role assignment、mapping、生成command、DCS destination | Manager mapping / DCS-BIOS command | Pico再flash、回路変更 |
| Pico が起動・通信しない | power、probe info、reset、RTT/defmt、USB/UART信号 | firmware / board / wiring | DCS、DCS-BIOS |
| 特定buttonだけ反応しない・複数同時に反応する | matrix row/column、continuity、short、scan log | GPIO / 回路 / firmware scan | DCS、DCS-BIOS、Manager（raw event前） |

## ユーザーに依頼する操作

ユーザー操作は、何を判定するためか、期待結果、観測方法をセットで依頼します。一度に複数の境界を変えないでください。

| 操作 | 依頼する条件 | 依頼時に明示すること |
| --- | --- | --- |
| DCS で mission を開始 | aircraft export を観測する場合 | 使用aircraft、開始後の待ち時間、見るstatus/log |
| cockpit control を1回動かす | export value または command 成功を証明する場合 | 対象control、変更前後、元に戻す必要性 |
| Manager の start/stop・設定変更 | Manager listener/config を対象にする場合 | 変更するendpoint、再起動要否、採取するexact error |
| USB の抜き差し・電源再投入 | enumeration/disconnect recovery の確認 | 対象device/port、実施順、ログ採取開始時点 |
| button/switch 操作 | event path を追う場合 | device、button位置、押す/離す、回数、同時押し禁止など |
| SWD 配線・probe接続 | Pico target/debug の確認 | board、probe、SWDIO/SWCLK/GND、給電方法 |
| firmware 書き込み | binary不一致または修正buildの実機確認 | 対象board、crate/ELF、変更内容、既存firmwareを上書きすること |
| 回路の導通・電圧確認 | software境界まで信号が来ない場合 | 電源ON/OFF、測定点、基準GND、期待範囲 |
| 配線・部品変更 | schematic/PCB と実機の不一致が証明された場合 | 必ず電源を切ること、変更箇所、復旧方法、再確認項目 |

- 読み取りだけの調査で済む間は、DCS cockpit state、firmware、配線を変更しません。
- DCS-BIOS command は送信成功だけでは確認になりません。DCS 上の状態または対応する export value で証明します。
- `probe-rs download --verify` は flash 内容の確認であり、機能確認ではありません。reset 後に LED、GPIO、UART、USB、RTT/defmt など仕様上の出力を観測します。
- 通電したまま配線や部品を変更するよう依頼しません。電圧測定が必要な場合は、測定点と安全条件が明確なときだけ依頼します。

## 切り分けの進め方

1. 症状を1つに絞り、上の表から最初の観測点を選びます。
2. software-only の test、保存済みlog、設定、read-only probe で確認できる範囲を先に調べます。
3. 経路の上流から、各境界について「入力あり / 出力あり / 未観測」を記録します。
4. 最初に「入力あり・出力なし」になった component を調査scopeにします。
5. そのcomponentだけを再現する最小構成を作ります。DCS側の問題にPicoを、device discoveryの問題にDCSを持ち込まないでください。
6. 実機操作や状態変更が必要になった時点で、ユーザーへ目的と手順を依頼します。
7. 修正後は局所testから始め、最後に1段上流と1段下流を含むintegrationを確認します。

調査報告では、観測事実、未確認事項、現在のscope、除外できたcomponent、次に必要な操作を分けて記載してください。

## 詳細手順

- DCS-BIOS の install / export / network / command: [`../.agents/skills/dcs-bios-connection/SKILL.md`](../.agents/skills/dcs-bios-connection/SKILL.md)
- DCS-BIOS 接続runbook: [`../.agents/skills/dcs-bios-connection/references/connection-debug.md`](../.agents/skills/dcs-bios-connection/references/connection-debug.md)
- Manager integration: [`../.agents/skills/dcs-bios-connection/references/homecockpit.md`](../.agents/skills/dcs-bios-connection/references/homecockpit.md)
- IMCP wire / address / retry / state: [`../.agents/skills/imcp-protocol/SKILL.md`](../.agents/skills/imcp-protocol/SKILL.md)
- Pico / Pico 2 build・SWD・flash・reset: [`../.agents/skills/pico-rust-probe-rs/SKILL.md`](../.agents/skills/pico-rust-probe-rs/SKILL.md)
- Upper Panel DDI firmware 実行手順: [`../firmware/upper_panel_ddi/README.md`](../firmware/upper_panel_ddi/README.md)
- HCP packet model: [`../firmware/hcp/README.md`](../firmware/hcp/README.md)
- CI での静的検証: [`../.github/workflows/ci.yml`](../.github/workflows/ci.yml)
