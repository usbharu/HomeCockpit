# IMCP-CLI再現シナリオ

以下は、リポジトリルートで`imcp-cli`を実行する例である。実行ファイルを使う場合は`$cli = '.\\utils\\target\\debug\\imcp-cli.exe'`、Cargo経由なら各コマンドを`cargo run --manifest-path utils/Cargo.toml --`に置き換える。

## 既知のフレームを作る

```powershell
$cli = '.\\utils\\target\\debug\\imcp-cli.exe'

# client(0x00)からmaster(0x01)へのJOIN
& $cli pack --from 0x00 --to 0x01 --packet-type join --id 0xCAFEBABE
# client(0x02)からmaster(0x01)へのPING
& $cli pack --from 0x02 --to 0x01 --packet-type ping
# client(0x02)からmaster(0x01)へのSetAddress ACK
& $cli pack --from 0x02 --to 0x01 --packet-type ack --address 0x00
# client(0x02)からmaster(0x01)へのSET
& $cli pack --from 0x02 --to 0x01 --packet-type set --data FE00
```

作ったhexは、送信前に次で確認する。

```powershell
& $cli unpack --format json --data FE0100030400BEBAFDDECA36FF
```

## JOIN成功シーケンス

stdinはワイヤ上のhexを1行ずつ受け取る。JOIN後にclient ACKを遅延して渡すと、ACK到着後に再送が止まる。

```powershell
$join = 'FE0100030400BEBAFDDECA36FF'
$ack = 'FE01020201000000FF'

& { Write-Output $join; Start-Sleep -Milliseconds 100; Write-Output $ack } |
  & $cli master --stdin --format json
```

期待する観測:

- `rx frame_type=join`が1件
- `tx event=bytes`のSetAddressが1件
- `rx frame_type=ack payload.address=0`が1件
- ACK後にSetAddressが追加出力されない

## ACK紛失と再送

ACKを渡さずEOFにすると、SetAddressは初回を含めて3回出力され、その後stdinモードが終了する。

```powershell
@($join) | & $cli master --stdin --format json
```

serialモードではプロセスは終了せず、同じ再送上限後にそのassignmentを破棄して受信待ちへ戻る。再送周期や上限を変更した場合は、期待値を実装の定数に合わせる。

## PINGとSETの応答

master自身のアドレスは`0x01`なので、masterが応答する入力はmaster宛てにする。

```powershell
$ping = & $cli pack --from 0x02 --to 0x01 --packet-type ping
$set = & $cli pack --from 0x02 --to 0x01 --packet-type set --data FE00

@($ping) | & $cli master --stdin --format json
@($set) | & $cli master --stdin --format json
```

PINGにはmaster(0x01)からclient(0x02)へのPONG、SETにはmasterからclientへのACKが出る。master宛てでないPINGは受信フレームとして表示されるだけで、応答しない。

## 分割入力と破損復帰

パーサーはstdinの行を単なるchunkとして扱うため、1フレームを複数行に分けても再構成できる。

```powershell
& { Write-Output 'FE0100030400'; Start-Sleep -Milliseconds 50; Write-Output 'BEBAFDDECA36FF' } |
  & $cli master --stdin --format json
```

チェックサムを壊したフレームの後に有効なPINGを渡し、errorの後にPONGが出ることを確認する。

```powershell
$badJoin = 'FE0100030400BEBAFDDECA37FF'
$ping = & $cli pack --from 0x02 --to 0x01 --packet-type ping
& { Write-Output $badJoin; Write-Output $ping } |
  & $cli master --stdin --format json
```

## masterからの発信

起動時の発信は`--send`を繰り返し指定する。PINGのようにACK待ちにならないフレームは順番に出力される。

```powershell
$ping = 'FE020100000003FF'
@() | & $cli master --stdin --format json --send $ping --send $ping
```

実機接続中の発信は次の形で行う。ポート名は`watch --list --format json`の結果から選び、ユーザーの依頼なしに送信しない。

```powershell
& $cli master --port COM3 --format json --control-stdin
send FE020100000003FF
```

## DATAとスタッフィング

DATAはmasterの自動応答対象ではない。予約バイトを含むpayloadを`pack`し、`unpack`でワイヤ上のESC処理と復元payloadを確認する。

```powershell
$data = & $cli pack --from 0x01 --to 0x02 --packet-type data --data FEFDFD00
& $cli unpack --format json --data $data
```

## 最小の診断記録

報告には次を残す。

- 実行モード（`watch`、`master --stdin`、`master --port`）とCLI引数
- 入力フレームの生成コマンドまたはhex
- `rx`/`tx`のJSON Linesと、再送回数・ACK payload
- 実機ポート、baud、実機送信の有無
- `cargo test`、clippy、rustfmtの結果（コード変更時）
