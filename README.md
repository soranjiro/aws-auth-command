# awx — Quick usage (concise)

Below are the minimal usage instructions and fictional examples only.

## Installation

You don't need Rust. We provide prebuilt binaries via Homebrew and npm.

See [INSTALL.md](INSTALL.md) for detailed installation instructions.

Quick install:
```sh
# Homebrew
brew tap soranjiro/awx https://github.com/soranjiro/aws-auth-command
brew install awx

# npm
npm install -g @soranjiro/awx

# Cargo
cargo install --git https://github.com/soranjiro/aws-auth-command
```

## Usage

```sh
awx [COMMAND] [OPTIONS] -- [AWS_COMMAND]...
```

Commands:
- `login`: Login to a specific profile and output environment variables to set
- `run`: Run AWS command with profile (default if no command specified)

Key options (short)

- `-p, --profile <PROFILE>`: Specify AWS profile to use
- `-c, --config`: Show discovered profiles (SSO/MFA/ROLE/STATIC)
- `-n, --no-interactive`: Non-interactive mode (CI)
- `--clear-cache [profile|all]`: Clear cache (no-op in MVP)

Examples (fictional outputs)

1) Login to an SSO profile

```sh
$ awx login -p sso-work
SSO token is not valid. Running: aws sso login --profile sso-work
SSO login completed for profile 'sso-work'.
# AWS credentials for profile 'sso-work' are ready.
# Copy and paste the following commands into your terminal:
export AWS_PROFILE=sso-work
# After setting the variables above, you can run AWS commands.
```

2) List S3 with a static profile that requires MFA

```sh
$ awx run -p example-profile -- s3 ls
✔ Select profile · example-profile [MFA][STATIC]
✔ Enter MFA code (6 digits) for arn:aws:iam::<ACCOUNT_ID>:mfa/test-user: · ******
> aws s3 ls
2025-10-16  my-bucket
```

3) Run an EC2 query with an SSO profile (auto runs `aws sso login` if needed)

```sh
$ awx -p sso-work -- ec2 describe-instances
SSO token missing. Running: aws sso login --profile sso-work
SSO login completed.
> aws ec2 describe-instances
{ ... }
```

4) CI / non-interactive failure when SSO login is required

```sh
$ AWX_NO_INTERACTIVE=1 awx -p production -- s3 ls
Error: SSO login required for profile "production". Run: aws sso login --profile production
Exit code: 2
```

5) Show discovered profiles (example output)

```sh
$ awx --config
dev       [STATIC]
staging   [SSO][ROLE]
prod      [SSO][ROLE][MFA]
```

Note: All outputs are fictional. Actual profile names, ARNs, and results depend on your environment.
