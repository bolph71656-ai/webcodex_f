# WebCodex

[日本語](README.md) | [English](README.en.md) | [简体中文](README.zh-CN.md)

**WebCodex は、ChatGPT、Claude、その他の AI エージェントが、あなた自身のコンピューター上にあるコードや開発ツールを直接扱えるようにするためのツールです。**

リポジトリの調査、コードの変更、テストの実行、Git の操作、不具合の調査などを AI アシスタントに依頼できます。リポジトリは現在あるコンピューター上にそのまま置いておけるため、AI コーディングエージェントを使うためだけにプロジェクトをホスト型ワークスペースへ移す必要はありません。

## WebCodex を使い始める

### 日常的な開発：通常の WebCodex（推奨）

ChatGPT から実際の開発環境を継続的に利用したい場合は、**通常のサーバー + ランナー**構成から始めてください。これは WebCodex の完全な開発体験で、複数プロジェクトへの継続的なアクセス、プロジェクト探索、編集、Git、コマンド、テスト、長時間実行される処理、コードナビゲーションを利用できます。公開 HTTPS、Cloudflare Tunnel、OpenAI Secure MCP Tunnel は、ChatGPT からサーバーへ到達するための接続方法にすぎず、別の制限付きモードへ切り替えるものではありません。

Windows または macOS では、**WebCodex Desktop + 公式 OpenAI Secure Tunnel** を最初の構成として推奨します。[デスクトップ版インストールガイド](docs/desktop-install.md)に従ってください。CLI、既存サーバー、セルフホスト、高度な構成については、[フルセットアップガイド](docs/PERSONAL_SETUP.md)を参照してください。

### 数分だけ試す：一時共有

WebCodex が自分のワークフローに合うかを手軽に確認するには、対象リポジトリ内で次を実行します。

```bash
cd /path/to/your/repository
npx --yes @yyjeqhc/webcodex share
```

`share` は通常の WebCodex Adaptive Runtime を利用した、一時的な単一プロジェクト用インスタンスを起動し、ChatGPT の接続情報を表示します。一時 Project Credential によりアクセスはその ProjectGrant の範囲に制限され、コマンドを終了するとエンドポイントと認証情報も無効になります。日常利用の標準構成ではなく、試用や短時間の共有を目的とした機能です。具体的な手順は [クイックトライアル](docs/QUICK_START.md)を参照してください。

## 何ができるのか

- **コードの理解と編集** — 設定済みプロジェクト内で、読み取り、検索、調査、安全性を考慮した変更を行えます。
- **実際のツールチェーンを利用** — リポジトリがあるコンピューター上で、コマンド、テスト、フォーマッター、コンパイラー、プロジェクト固有のツールを実行できます。
- **Git を利用** — リポジトリ操作を確認可能な状態に保ちながら、ステータスや差分を調査できます。
- **長時間処理に対応** — 1 回のモデル応答を開き続ける必要なく、ジョブの状態を確認できます。
- **人によるレビューを支援** — [Runtime Console](docs/runtime-console.md)、Workflow Session の証跡、Jobs、Git/差分レビューを利用できます。

## WebCodex を使う理由

- **コードを自分のコンピューター上に保持できます。** リポジトリをチャットサービスへコピーする必要はありません。
- **AI エージェントが実際の開発環境を利用できます。** 普段使っているファイル、Git チェックアウト、コンパイラー、テスト、各種ツールをそのまま利用できます。
- **作業を 1 回の依頼より長く継続できます。** 長時間実行される処理とその証跡を WebCodex 上で確認できます。
- **一時利用から常設運用まで対応できます。** 1 コマンドの共有で短時間試すことも、セルフホストしたサーバーへ複数のコンピューターを接続して継続運用することもできます。

## 仕組み

```text
AI クライアント
   |
   | MCP / HTTPS
   v
WebCodex
   |
   v
あなたのコンピューター
   |
   +-- リポジトリ
   +-- Git
   +-- コンパイラー / テスト / 開発ツール
```

内部のサーバー/ランナー構成、プロトコル面、権限境界については、[アーキテクチャ](docs/ARCHITECTURE.md)、[MCP](docs/MCP.md)、[認証](docs/AUTH_MODEL.md)を参照してください。

## Star History

[![Star History Chart](https://api.star-history.com/image?repos=yyjeqhc/webcodex&type=Date)](https://www.star-history.com/yyjeqhc/webcodex)

## 対応プラットフォーム

- **Linux x64/arm64** — ローカル `share`、サーバー、ランナーの各ワークフロー。
- **macOS x64/arm64** — デスクトップ版のローカルサーバー + ランナー、OpenAI Secure Tunnel、ローカル `share`、単独ランナー。
- **Windows x64** — デスクトップ版のローカルサーバー + ランナーと公式 OpenAI Secure Tunnel、CLI + ランナー、ローカルのフォアグラウンドサーバー、明示的な `webcodex share --tunnel cloudflare|openai|none`。
- **Windows arm64** — CLI + ランナー、ローカルのフォアグラウンドサーバー、`share`。管理対象 OpenAI `tunnel-client` に対応しています。固定されている Cloudflare リリースには公式 Windows ARM64 アーティファクトがないため、Cloudflare を使う場合は信頼できる明示指定または PATH 上の `cloudflared` が必要です。デスクトップ版インストーラーは現在 Windows x64 のみです。デスクトップ版が所有するフォアグラウンド実行環境以外では、WebCodex 管理の Windows サーバーサービスは未対応です。

Windows と長期運用については、[Deployment](docs/DEPLOYMENT.md) と [MCP](docs/MCP.md) を参照してください。

## 既存サーバーと高度な設定

すでに WebCodex サーバーと接続用認証情報が提供されている場合は、その既存サーバーを利用し、[フルセットアップガイド](docs/PERSONAL_SETUP.md)に従ってください。通常の Windows/macOS 個人利用では、[デスクトップ版ガイド](docs/desktop-install.md)を使用してください。[Deployment](docs/DEPLOYMENT.md) は、本番ホスティング、複数ユーザー、systemd/Docker、OAuth、プロキシ、プライベート CA などの用途向けです。

これらは初回利用時に理解しておくべき必須概念ではなく、運用開始後に必要に応じて扱う項目です。

## ドキュメント

- [デスクトップ版インストール](docs/desktop-install.md) — Windows/macOS 向け推奨構成：デスクトップ版 + 公式 OpenAI Secure Tunnel
- [デスクトップ版の使い方](docs/desktop-guide.md) — プロジェクト、接続、アクティビティ、バックグラウンド動作
- [フルセットアップ](docs/PERSONAL_SETUP.md) — CLI、既存サーバー、Linux、高度な通常サーバー + ランナー構成
- [クイックトライアル](docs/QUICK_START.md) — `share` で 1 リポジトリを一時的に試す
- [MCP](docs/MCP.md) — ChatGPT、Claude、認証方式、MCP リファレンス
- [Deployment](docs/DEPLOYMENT.md) — 本番運用、セルフホスト、高度な運用
- [トラブルシューティング](docs/TROUBLESHOOTING.md) — 接続と実行環境の問題
- [CLI](docs/CLI.md) — コマンドと認証情報のリファレンス
- [AI 支援セットアップ](docs/AI_ONBOARDING.md) — AI エージェントに WebCodex の設定を支援してもらう
- [セキュリティ](SECURITY.md) — セキュリティモデルと運用上の注意
- [ドキュメント一覧](docs/INDEX.md) — 利用者・コントリビューター向けドキュメント一覧

## セキュリティ

WebCodex は、設定済みプロジェクトの境界内でファイルを読み書きし、コマンドを実行できます。バージョン管理を利用し、認証情報をプロンプト・ログ・Git に含めず、AI アシスタントからアクセスさせてよいプロジェクトルートだけを登録してください。完全なセキュリティモデルは [SECURITY.md](SECURITY.md) を参照してください。

## ソースからビルド

```bash
cargo build --release --workspace --bins
export PATH="$PWD/target/release:$PATH"
```

## コントリビューション

WebCodex 自身や他のコーディングエージェントを利用して作成した変更も含め、コントリビューションを歓迎します。不具合報告、開発フロー、プルリクエストのガイドラインについては [CONTRIBUTING.md](CONTRIBUTING.md) を参照してください。

## 謝辞

技術的な議論とオープンソース共有を支える場を提供している [LINUX DO](https://linux.do/) コミュニティに感謝します。

## ライセンス

Apache License, Version 2.0 で提供されます。詳細は [LICENSE](LICENSE) を参照してください。
