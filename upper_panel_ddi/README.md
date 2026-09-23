# Upper Panel DDI

Upper Panel DDI に関する基板設計、機械設計、ファームウェアの対応関係をまとめます。

## 構成

| 対象 | リポジトリ内の場所 | 対応する実装 |
| --- | --- | --- |
| メイン基板 | [`pcb/main_board/`](pcb/main_board/) | [`../firmware/upper_panel_ddi/`](../firmware/upper_panel_ddi/) が搭載MCU上で動作 |
| ボタン基板 | [`pcb/button_panel/`](pcb/button_panel/) | 独立したファームウェアはなく、メイン基板へ接続して使用 |
| ケース・機械部品 | [`mechanical/case/`](mechanical/case/) | メイン基板とボタン基板を組み込む筐体。3Dモデルは将来 `mechanical/case/3d/` に配置 |
| ホスト側モック | [`../utils/upper-panel-ddi-mock/`](../utils/upper-panel-ddi-mock/) | Managerから実機相当のIMCP/HCPデバイスとして利用 |

KiCadプロジェクトのファイル名と内部プロジェクト名は、既存の参照や製造手順との互換性のため維持しています。

## 検証

CIではメイン基板とボタン基板それぞれについて、KiCadのERC/DRCを実行します。ファームウェアのビルド・書き込み手順は [`firmware/upper_panel_ddi/README.md`](../firmware/upper_panel_ddi/README.md) を参照してください。
