# Upper Panel DDI

Upper Panel DDI に関する基板設計、機械設計、ファームウェアの対応関係をまとめます。

## 構成

| 対象 | リポジトリ内の場所 | 対応する実装 |
| --- | --- | --- |
| メイン基板 | [`pcb/main_board/`](pcb/main_board/) | [`../firmware/upper_panel_ddi/`](../firmware/upper_panel_ddi/) が搭載MCU上で動作 |
| ボタン基板 | [`pcb/button_panel/`](pcb/button_panel/) | 独立したファームウェアはなく、メイン基板へ接続して使用 |
| ケース・機械部品 | [`mechanical/case/`](mechanical/case/) | メイン基板とボタン基板を組み込む筐体。3Dモデルは将来 `mechanical/case/3d/` に配置 |
| ホスト側モック | [`../utils/upper-panel-ddi-mock/`](../utils/upper-panel-ddi-mock/) | Managerから実機相当のIMCP/HCPデバイスとして利用 |

メイン基板 KiCad プロジェクト: [`pcb/main_board/upper_panel_ddi_main_board.kicad_pro`](pcb/main_board/upper_panel_ddi_main_board.kicad_pro)（回路図のみ。配線は [`firmware/upper_panel_ddi`](../firmware/upper_panel_ddi/) に合わせ、[`pcb/main_board/generate_schematic.py`](pcb/main_board/generate_schematic.py) で `.kicad_sch` を再生成可能）。メイン基板はボタン基板と同じ JST 1×8（`J1`..`J8`、Pin1=行、Pin3–7=列バス、Pin2/8 NC）で 8 枚のボタン基板に接続します。ボタン基板: [`pcb/button_panel/upper_panel_ddi_button_panel.kicad_pro`](pcb/button_panel/upper_panel_ddi_button_panel.kicad_pro)。

## 検証

CI ではメイン基板に `kicad-cli sch erc`（error 0）、ボタン基板に ERC/DRC を実行します。手順の詳細は [`docs/kicad.md`](../docs/kicad.md) を参照してください。ファームウェアのビルド・書き込みは [`firmware/upper_panel_ddi/README.md`](../firmware/upper_panel_ddi/README.md) を参照してください。
