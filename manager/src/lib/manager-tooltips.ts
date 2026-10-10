/** UI に常時出さない状態・指標の説明（HelpTip 用） */
export const managerTooltips = {
  headerDcsConnection:
    "DCS-BIOS ブリッジの接続状態。メトリクスと診断は「ソフトウェア接続」、イベントは「ログ」タブ。",
  logDcsSummary:
    "ログタブ用の DCS-BIOS 概要。詳細な接続設定・診断は「ソフトウェア接続」タブ。",

  dcsMetricConnection: "Export 受信とコマンド経路のランタイム状態。",
  dcsMetricRate: "直近の export パケット受信レートと累計。",
  dcsMetricLastSeen: "最後に export を受信した時刻。下段はコマンド送信先。",
  dcsMetricAircraft: "Export から得た機体名。「Adapter設定」でプロファイル照合に使用。",
  dcsDiagnostics: "Bind・multicast・通信有無など、接続まわりの診断メッセージ。",

  mcpServer:
    "DCS-BIOS とは別経路。ローカルの MCP クライアントが Manager とデバイスを操作。認証なし。",

  deviceAutoSave: "Endpoint の編集は短い待機後に自動保存されます。",
  deviceCandidates:
    "115200 baud で応答した未登録ポート。候補を選んでも「追加」するまで登録されません。",
  endpointRoleHint: "Hub 配下と直結デバイスの探索時に使うヒント。",

  mappingContinuousLearn:
    "選択 Device の未結線 logical control を順に学習。全件クリアしての再学習も可能。",
  mappingContinuousLearnPhase:
    "arming: 次項目の学習を開始中 / waiting: 物理入力待ち / paused: 中断（再開可） / completed: キュー完了",

  adapterConnection: "DCS-BIOS ブリッジの接続状態（Adapter の入力元）。",
  adapterCurrentAircraft: "Export の機体名。保存済みプロファイル名と照合。",
  adapterCatalog: "内蔵カタログとユーザー保存プロファイルの一覧。",
  adapterUnknownAircraft: "一致プロファイルがない間は Adapter 向け I/O マッピングは生成されません。",

  roleIoPendingProfile: "機体プロファイルが確定すると Role ごとの DCS-BIOS I/O を設定できます。",
} as const;

export type ManagerTooltipKey = keyof typeof managerTooltips;
