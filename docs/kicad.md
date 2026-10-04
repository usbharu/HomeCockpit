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

変換後は KiCad 10 で対象プロジェクトを開いて保存し、ERC/DRC と SVG 出力を確認します。プロジェクト内の `sym-lib-table` / `fp-lib-table` は、標準ライブラリを KiCad 10 の環境変数（例: `KICAD10_SYMBOL_DIR`）から、共有ライブラリを `${KIPRJMOD}` から解決するために使用しています。個人環境の絶対パスをテーブルへ追加しないでください。旧ルートの `upper_panel_ddi/upper_panel_ddi.*` メイン基板は削除済みです。正本は `upper_panel_ddi/pcb/main_board/upper_panel_ddi_main_board.kicad_pro` と同ディレクトリの `.kicad_sch` です。

## 重複パッドを持つタクトスイッチ

`SW_PUSH-12mm-mini` は同じ電気接点に属するパッド番号 1 と 2 を持つ部品です。KiCad 10 の `duplicate_pad_numbers_are_jumpers yes` と `duplicate_pin_numbers_are_jumpers yes` をライブラリ、基板、回路図の対応箇所に設定しています。これは実部品の内部接続を記述するもので、DRC/ERC の警告を一括無効化する設定ではありません。

## メイン基板の回路図を変更する

メイン基板は A3 縦の1枚に、Pico と8個のパネルコネクタを配置しています。ROW0–7 は各コネクタへ個別に配線し、COL0–4 は5本の共通配線から各コネクタへ分岐します。グローバルラベルは使用していません。各ネットに1個ずつ置いたローカルラベルは、連続した配線の名前を示します。交差する線は接続点のある箇所だけで接続します。

変更時は KiCad 10.0.5 以降で [`upper_panel_ddi_main_board.kicad_pro`](../upper_panel_ddi/pcb/main_board/upper_panel_ddi_main_board.kicad_pro) を開き、回路図エディターで `.kicad_sch` を編集・保存します。生成スクリプトは使用しません。LLMが編集する場合も `.kicad_sch` を正本として扱います。

Pico・コネクタ・電源記号はKiCad標準ライブラリのシンボルです。シンボル定義は通常のKiCad回路図と同様に `.kicad_sch` 内にも保存されているため、別のシンボル抽出ファイルは不要です。ERCレポート、ネットリスト、描画画像は一時ディレクトリへ出力してください。

保存後は下記のERCを実行し、SVGまたはPNGで配線の重複、部品・文字との重なり、分岐の接続点を確認します。既存の配線を描き直す場合は、変更前後のネットリストで接続先のピンが一致することも確認してください。

## CI と製造データの確認

PRで `upper_panel_ddi/pcb/**` など KiCad 関連ファイルを変更すると、`KiCad previews` が対象設計を自動検出し、変更前後の画像を「KiCad 変更プレビュー」コメントに表示します。回路図のみの新規プロジェクト（例: `upper_panel_ddi/pcb/main_board/`）も検出対象です。基板は表面と裏面、回路図は全ページをPNGで表示します。基板の画像は配線・パッド・シルク・外形を確認するための2D表示です。

比較元はPRの分岐点、比較先はPRの最新コミットです。追加した設計は変更後、削除した設計は変更前のみ表示し、更新時には同じコメントを書き換えます。共有シンボル・フットプリント・ライブラリテーブルの変更では全設計を再描画します。PNGと元のSVGはActionsの `kicad-previews` artifactからダウンロードできます。画像生成に失敗した場合も、生成済みの画像と失敗した対象をコメントに残します。Docker 内の `kicad-cli` 描画では、メイン基板 ERC と同様に `KICAD10_SYMBOL_DIR` を設定し、プロジェクトディレクトリを `KIPRJMOD` として渡して `sym-lib-table` / `fp-lib-table` を解決します。ワークフローは `upper_panel_ddi/pcb/**` など KiCad 関連パスが変わった PR のみ実行します（`ci.yml` の `kicad_button` / `kicad_main` と同系統）。

このコメントはERC／DRCの成否にかかわらず生成され、既存の違反コメントとは別に管理します。画像は専用の `ci-previews/pr-<PR番号>` ブランチに保存します。書き込み権限の制約により、forkからのPRでは画像生成とartifact保存まで行い、コメントは投稿しません。

描画ワークフローは読み取り権限で実行し、その完了を受けた `KiCad preview comment` が投稿します。投稿側のワークフローとスクリプトは既定ブランチのコミットを使用し、PRのコードを実行しません。artifactは別のディレクトリに取得し、実行元PR・コミット・画像ファイルを検証してから投稿します。投稿ワークフローはmasterにマージした後に有効になります。

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

CI の error-level JSON と SVG 可視化を最終判定に使用します（メイン基板は ERC のみ）。GitHub Actions では `kicad-main-board` job が保存された `.kicad_sch` に対して `kicad-cli sch erc`（error 0）と回路図 SVG を検証します。`upper_panel_ddi/pcb/main_board/`、`firmware/upper_panel_ddi/`、共有 KiCad ライブラリの変更でこの job が走ります。

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
