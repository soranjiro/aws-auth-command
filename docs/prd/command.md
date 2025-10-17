AWS Wrapper CLI (`awx`) コマンド一覧

- `awx help`
  - `awx`コマンドのヘルプを表示する

- `awx login`
  1. awsのプロファイルを選択
  2. そのプロファイルの認証情報を取得
    a. ssoならaws sso loginでログインを行ってしまう
    b. mfaならmfaコードを入力させてログインをする
    c. その他認証に必要なものを取得
  3. 認証情報（credentialsで必要なもの。profileやaccess key id, secret access keyなど）を`export`コマンドを使ってそのターミナルの環境変数化する。
  4. 以降の`awx`コマンドやAWS CLIコマンドはその環境変数を使って認証を行う（つまり、`aws`コマンドを実行する前に`awx login`を実行すると認証情報がセットされる）

  - `-p | --profile [profile_name]`
    - 使用するAWSプロファイルを指定する
    - 指定しない場合、デフォルトプロファイルが使用される

- `awx {command}`
  - AWS CLIのコマンドを実行する
  - ログインしていなければ（環境変数に設定されていなければ）、ログイン（`awx login`）を実行する
  - 例:
    - `awx s3 ls [bucket_name]`
      - 指定したバケットの中身を一覧表示する
    - `awx s3 cp [source] [destination]`
      - S3とローカル間でファイルをコピーする
    - `awx ec2 describe-instances`
      - EC2インスタンスの情報を取得する
