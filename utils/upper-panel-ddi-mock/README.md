# Upper Panel DDI mock

macOS/Linux の POSIX PTY 上で Upper Panel DDI を再現する開発用 CLI です。
Manager からは通常の直結 IMCP/HCP デバイスとして見えます。

```sh
cd utils
cargo run -p upper-panel-ddi-mock -- --device-id 0xDD10000000000001
```

起動時に表示される PTY のパスを Manager のシリアル endpoint に指定します。
Baud Rate は `115200`、Role Hint は `自動` または `直結デバイス` を選択してください。

対話コマンド:

- `press <control-id>`: 0–39 のボタンを押す
- `release <control-id>`: ボタンを離す
- `tap <control-id>`: 押下と解放を順番に送る
- `status`: 接続状態と押下中のボタンを表示する
- `help`: コマンド一覧を表示する
- `quit`: 終了する

PTY はプロセス終了時に消えます。再起動後は、新しく表示されたパスを Manager に設定し直してください。
