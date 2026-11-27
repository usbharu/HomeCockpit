# リリース手順

Manager デスクトップアプリのリリースは、`v*` 形式のタグ push を起点に [`.github/workflows/release.yml`](../.github/workflows/release.yml) が自動で実行されます。このドキュメントでは、リリースに必要なバージョン更新からタグ push までの手順を説明します。

## リリースフロー

1. バージョンを更新する（後述の4ファイル）
2. 変更を PR にして `master` へマージする
3. `master` の CI が成功していることを確認する
4. `master` の対象コミットにタグを作成して push する
5. GitHub Actions がビルド・Release 作成・インストーラ添付まで自動で行う

Release ワークフローはビルドのみを行い、テストは実行しません。`master` の CI が成功したコミットにタグを打ってください。

## バージョン更新

以下の4箇所を同じバージョンに揃えます。不一致がある場合、Release ワークフローはビルド前に失敗します。

| ファイル | 項目 |
| --- | --- |
| [`manager/src-tauri/tauri.conf.json`](../manager/src-tauri/tauri.conf.json) | `version` |
| [`manager/src-tauri/Cargo.toml`](../manager/src-tauri/Cargo.toml) | `[package] version` |
| [`manager/src-tauri/Cargo.lock`](../manager/src-tauri/Cargo.lock) | `manager` パッケージの `version` |
| [`manager/package.json`](../manager/package.json) | `version` |

`Cargo.lock` は手編集しません。`Cargo.toml` を更新した後、`manager/src-tauri` で Cargo コマンドを実行して更新します。

```bash
cd manager/src-tauri
cargo update -p manager
git diff -- Cargo.lock
cargo metadata --locked --format-version 1 > /dev/null
```

`git diff` で `manager` のバージョンだけが変わっていることを確認してください。dependency の更新が混ざった場合はコミットしないでください。

`manager/package.json` のバージョンは pnpm の lockfile には影響しません。

## タグの作成と push

Release ワークフローは `origin/master` の祖先でないコミットのタグを拒否します。未マージのコミットにタグを打つと検証で失敗します。

```bash
git switch master
git pull --ff-only origin master
git tag -a v0.2.0 -m "v0.2.0"
git push origin v0.2.0
```

バージョンに `-` を含めるとプレリリースとして扱われます。

```bash
git tag -a v0.2.0-rc.1 -m "v0.2.0-rc.1"
git push origin v0.2.0-rc.1
```

プレリリースは GitHub の "Latest" に設定されません。プレリリースでも、4ファイルのバージョンはタグと同じ `0.2.0-rc.1` に揃えてください。

このワークフローはタグの push を起点に動作します。GitHub の画面からタグを作成するのではなく、上記の `git push` 手順を使用してください。

## ワークフローの動作

### 検証

tag 名、4ファイルのバージョン一致、`master` の祖先であることを検証します。タグ名は `vX.Y.Z` または `vX.Y.Z-suffix` のみ許可されます。

### ビルド

| プラットフォーム | runner | 成果物 |
| --- | --- | --- |
| macOS universal | `macos-latest` | `.dmg` |
| Windows x64 | `windows-latest` | `.msi`, `-setup.exe` |
| Linux x64 | `ubuntu-22.04` | `.AppImage`, `.deb`, `.rpm` |

macOS は ad-hoc 署名（`APPLE_SIGNING_IDENTITY=-`）のみ行います。Apple Developer 証明書による署名・公証は行いません。Windows も未署名です。

### Release の作成

1. 全プラットフォームの成果物が揃ったことを確認する
2. `SHA256SUMS` を生成する
3. 下書き Release を作成する
4. インストーラと `SHA256SUMS` を添付する
5. すべての添付が成功した後に Release を公開する

リリースノートは GitHub の `--generate-notes` で生成されます。前回の Release 以降に `master` へマージされた PR が自動で列挙され、冒頭にインストール手順が付加されます。初回リリースでは前回の Release が無いため、リポジトリ全体のマージ済み PR が対象になります。

PR にラベルを付けている場合でも、このリポジトリでは `.github/release.yml` による分類は行っていません。依存更新の PR も一覧に含まれます。

### 再実行

Actions の Release ワークフローから "Re-run all jobs" を実行できます。

- 下書き Release が残っている場合は再利用され、添付ファイルが上書きされます
- 公開済み Release が存在する場合は、タグを再利用せず失敗します。バージョンを上げて新しいタグを push してください

### 失敗した Release の削除

プレリリースの動作確認などで Release とタグを削除する場合:

```bash
gh release delete v0.2.0-rc.1 --yes --cleanup-tag
```

`--cleanup-tag` はリモートのタグも削除します。ローカルのタグも不要であれば `git tag -d v0.2.0-rc.1` で削除してください。

## リリース前チェックリスト

- [ ] 4ファイルのバージョンが一致している
- [ ] `Cargo.lock` の変更が `manager` のバージョンだけになっている
- [ ] バージョン更新 PR が `master` にマージ済み
- [ ] `master` の CI が成功している
- [ ] タグが `master` 上のコミットを指している
- [ ] プレリリースでは `-rc.1` などの suffix を付けている

## 将来の拡張

現在は署名・公証・自動アップデートを行いません。導入する場合は以下を追加します。

- macOS 署名・公証: `APPLE_CERTIFICATE`、`APPLE_CERTIFICATE_PASSWORD`、`APPLE_API_ISSUER`、`APPLE_API_KEY`、`APPLE_API_KEY_PATH`
- Tauri updater: `createUpdaterArtifacts` と署名鍵、`tauri-plugin-updater`
- Windows 署名: コードサイニング証明書と `signtool`

secret を追加する場合は、Release ワークフローから参照する環境変数と手順をこのドキュメントに追記してください。
