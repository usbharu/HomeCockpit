# KiCad の開発環境と移行手順

このリポジトリで管理するKiCad設計と共有ライブラリは KiCad 10 の形式を正本とします。最低対応バージョンは KiCad 10.0 です。CI は再現性のため `ghcr.io/kicad/kicad:10.0.5` を固定して使用するため、ローカルで編集する場合も KiCad 10.0.5 以降を推奨します。現行の KiCad 検証対象は次のとおりです。

| プロジェクト | パス | CI |
| --- | --- | --- |
| Upper Panel メイン基板 | `upper_panel_ddi/pcb/main_board/` | 回路図 ERC のみ（`.kicad_pcb` なし） |
| Upper Panel ボタン基板 | `upper_panel_ddi/pcb/button_panel/` | ERC + DRC + SVG |

## KiCad 9 からの移行

KiCad 10 は KiCad 9 とファイル形式の互換性がありません。移行前にブランチまたはバックアップを作成し、KiCad 10 で保存したファイルを KiCad 9 で再保存しないでください。

既存ファイルを移行する場合は、リポジトリのルートで次のコマンドを実行します。`upgrade` は現在の形式へ変換して保存します。

```sh
kicad-cli sch upgrade upper_panel_ddi/pcb/button_panel/upper_panel_ddi_button_panel.kicad_sch
kicad-cli pcb upgrade upper_panel_ddi/pcb/button_panel/upper_panel_ddi_button_panel.kicad_pcb
kicad-cli fp upgrade Library.pretty
kicad-cli sym upgrade HomeCockpit.kicad_sym
```

変換後は KiCad 10 で対象プロジェクトを開いて保存し、ERC/DRC と SVG 出力を確認します。プロジェクト内の `sym-lib-table` / `fp-lib-table` は、標準ライブラリを KiCad 10 の環境変数（例: `KICAD10_SYMBOL_DIR`）から、共有ライブラリを `${KIPRJMOD}` から解決するために使用しています。個人環境の絶対パスをテーブルへ追加しないでください。旧ルートの `upper_panel_ddi/upper_panel_ddi.*` メイン基板は削除済みです。正本は `upper_panel_ddi/pcb/main_board/upper_panel_ddi_main_board.kicad_pro` です。回路図は [`generate_schematic.py`](../upper_panel_ddi/pcb/main_board/generate_schematic.py) から再生成できます。

## 重複パッドを持つタクトスイッチ

`SW_PUSH-12mm-mini` は同じ電気接点に属するパッド番号 1 と 2 を持つ部品です。KiCad 10 の `duplicate_pad_numbers_are_jumpers yes` と `duplicate_pin_numbers_are_jumpers yes` をライブラリ、基板、回路図の対応箇所に設定しています。これは実部品の内部接続を記述するもので、DRC/ERC の警告を一括無効化する設定ではありません。

## CI と製造データの確認

静的検証は次の形式で実行します。

```sh
# メイン基板（schematic-only）
kicad-cli sch erc --severity-error --exit-code-violations \
  --output /tmp/main-board-erc.rpt \
  upper_panel_ddi/pcb/main_board/upper_panel_ddi_main_board.kicad_sch

# ボタン基板
kicad-cli sch erc --severity-all --output /tmp/button-panel-erc.rpt \
  upper_panel_ddi/pcb/button_panel/upper_panel_ddi_button_panel.kicad_sch
kicad-cli pcb drc --severity-all --output /tmp/button-panel-drc.rpt \
  upper_panel_ddi/pcb/button_panel/upper_panel_ddi_button_panel.kicad_pcb
```

CI の error-level JSON と SVG 可視化を最終判定に使用します（メイン基板は ERC のみ）。KiCad 10 への変換だけで、設計上の未接続や未配線を除外してはいけません。

製造データは生成物として管理し、コミット前に必要な差分だけを確認します。

```sh
mkdir -p /tmp/kicad-production/button-panel
kicad-cli pcb export gerbers --output /tmp/kicad-production/button-panel \
  upper_panel_ddi/pcb/button_panel/upper_panel_ddi_button_panel.kicad_pcb
kicad-cli pcb export drill --output /tmp/kicad-production/button-panel \
  upper_panel_ddi/pcb/button_panel/upper_panel_ddi_button_panel.kicad_pcb
kicad-cli pcb export pos --output /tmp/kicad-production/button-panel-pos.csv \
  --format csv --units mm upper_panel_ddi/pcb/button_panel/upper_panel_ddi_button_panel.kicad_pcb
```

移行確認はファイル形式と CI の再現性を対象とし、基板実物、部品実装、導通、製造業者の受け入れ確認は含みません。
