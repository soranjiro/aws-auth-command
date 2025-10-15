 # awx — 使い方（簡潔）

以下は本コマンドの最小限の使い方と、架空の例のみを示します。

使い方（構文）

```sh
awx [OPTIONS] -- [AWS_COMMAND]...
```

主要オプション（抜粋）

- `-p, --profile <PROFILE>`: 実行に使う AWS プロファイルを指定
- `-c, --config`: 発見したプロファイル（SSO/MFA/ROLE/STATIC）を表示
- `-n, --no-interactive`: 対話を一切行わず失敗する（CI 用）
- `--clear-cache [profile|all]`: キャッシュ削除（MVP では無効）

例（架空の出力）

1) MFA を要求する static プロファイルで S3 を一覧表示

```sh
$ awx -p example-profile -- s3 ls
✔ Select profile · example-profile [MFA][STATIC]
✔ Enter MFA code (6 digits) for arn:aws:iam::<ACCOUNT_ID>:mfa/test-user: · ******
> aws s3 ls
2025-10-16  my-bucket
```

2) SSO プロファイルでログインが必要な場合（自動的に `aws sso login` を起動）

```sh
$ awx -p sso-work -- ec2 describe-instances
SSO token missing. Running: aws sso login --profile sso-work
SSO login completed.
> aws ec2 describe-instances
{ ... }
```

3) 非対話モード（CI）で認証が必要な場合の失敗例

```sh
$ AWX_NO_INTERACTIVE=1 awx -p production -- s3 ls
Error: SSO login required for profile "production". Run: aws sso login --profile production
Exit code: 2
```

4) 発見したプロファイルの一覧表示（例）

```sh
$ awx --config
dev       [STATIC]
staging   [SSO][ROLE]
prod      [SSO][ROLE][MFA]
```

注意: 上記はすべて架空の例です。実際のプロファイル名・ARN・出力は環境に依存します。
