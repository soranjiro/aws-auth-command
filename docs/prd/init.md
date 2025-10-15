# PRD: awx – AWS Authentication Wrapper (Final)

## 1. Overview and Scope

Purpose: Provide a small wrapper around the AWS CLI that simplifies authentication and profile switching for real-world workflows while minimizing required user interaction and preserving the existing `aws` command experience. Support both interactive and non-interactive use.

Target: Users who operate across multiple products, organizations, and personal accounts where authentication methods mix: SSO (IAM Identity Center), MFA-protected IAM users, AssumeRole, and static credentials.

Non-goals: GUI, Windows support, custom long-term SSO token storage, or storing long-lived credentials in plaintext.

## 2. Core Value (UX)

- Ask for the minimum input required at the moment: select a profile → authenticate only if needed → run the command.
- Respect the AWS CLI ecosystem: delegate SSO to `aws sso login` and rely on the AWS CLI's official SSO cache; do not reimplement SSO token storage.
- Secure defaults: credentials are non-persistent by default; SSO tokens are not stored by `awx`.

## 3. Users and Assumptions

- Users manage multiple accounts/roles across projects and may have a mixture of:
    - AWS SSO (IAM Identity Center) profiles (e.g. `sso_start_url`)
    - IAM users with MFA (`mfa_serial` + static keys)
    - AssumeRole profiles (`role_arn` with `source_profile`)
    - Single-account usage with static keys
- Platforms: macOS and Linux. AWS CLI v2 is the expected runtime.

## 4. Functional Requirements (Minimal Set)

### 4.1 Profile detection and classification

- Read `~/.aws/config` and `~/.aws/credentials` (INI) and classify profiles as:
    - SSO profiles (presence of `sso_start_url`/`sso_region`)
    - AssumeRole profiles (`role_arn` with `source_profile`)
    - Static credential profiles (`aws_access_key_id`, `aws_secret_access_key`)
    - MFA-required profiles (`mfa_serial` present)
- Show an interactive list with badges for default/SSO/MFA/ROLE. Skip UI when `--profile` is provided.

### 4.2 Authentication strategy

- SSO: If not logged in or token is expired, start `aws sso login --profile <name>` (browser-based flow). `awx` relies on AWS CLI's SSO cache and does not persist SSO tokens itself.
- MFA + static keys: Prompt for a 6-digit code (no echo) and call `aws sts get-session-token --serial-number <mfa_serial> --token-code <code>`. If the profile requires role assumption, perform `sts assume-role` as needed.
- AssumeRole: Call `aws sts assume-role` using available base credentials (MFA step first if required).
- Static-only: Use static keys directly.

### 4.3 Session cache (secure opt-in)

- Default: no persistent cache (in-memory only).
- Opt-in: persist sessions only when explicitly enabled (`AWX_CACHE=1` or config setting).
    - SSO: `awx` does not persist SSO tokens; AWS CLI SSO cache is used.
    - STS temporary credentials: prefer OS keychain (via `keyring`). If keychain is unavailable, fall back to an encrypted file under `~/.awx/cache/` (permission 600) protected with authenticated encryption (ring), but fallback storage requires a passphrase.
    - Auto-expire: obey STS `Expiration`; detect expiry and automatically re-authenticate (interactive only).
    - Clear cache with `awx --clear-cache [profile|all]`.

### 4.4 Command execution

- Run `aws` as a child process and pass stdin/stdout/stderr through, preserving exit codes and forwarding signals (SIGINT/SIGTERM).
- Before execution, set environment variables for the child process: `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN`, and `AWS_DEFAULT_REGION` where appropriate.

### 4.5 Auxiliary features

- `--config`: show discovered profiles and status (SSO/MFA/ROLE badges, SSO login state, cache TTL).
- `--no-interactive` or `AWX_NO_INTERACTIVE=1`: non-interactive mode for CI. If authentication is required and cannot proceed non-interactively, fail with a clear exit code.
- `--version`, `--verbose`, `--clear-cache [profile|all]`.

## 5. CLI specification (concise)

```
awx [OPTIONS] -- [AWS_COMMAND]...

OPTIONS:
    -p, --profile <PROFILE>      Specify profile directly
    -c, --config                 Show configuration/status
            --clear-cache [TARGET]   Clear cache (profile name or 'all')
    -n, --no-interactive         Skip interactive UI (for CI)
    -v, --verbose                Verbose logging
    -V, --version                Show version

AWS_COMMAND: e.g. s3 ls, ec2 describe-instances ...
```

Note: If SSO login is required, `awx` will advise or launch `aws sso login --profile <name>` in interactive mode.

## 6. Execution flow (representative)

### 6.1 SSO profile

- Select profile (or use `--profile`). If an SSO token is valid, run the command; if expired or missing, run `aws sso login` (browser flow) then proceed.

### 6.2 MFA + AssumeRole

- Select profile → prompt for 6-digit MFA → `get-session-token` → `assume-role` → run command.

### 6.3 Non-interactive (CI)

- `--no-interactive --profile <name> -- <AWS_COMMAND>`. If SSO login or MFA input is required and cannot be supplied non-interactively, fail fast with a clear message and exit code 2.

### 6.4 Implementation sequence (detailed)

1) Startup and input parsing
- Parse `awx [OPTIONS] -- [AWS_COMMAND]...`.
- Resolve profile precedence: `--profile` > `AWS_PROFILE` env var > `default`.
- If `--no-interactive` is set, suppress all prompts.

2) Load and classify profiles
- Read `~/.aws/config` and `~/.aws/credentials` (INI parsing).
- Tag each profile with detected attributes: SSO (presence of `sso_start_url`/`sso_region`), AssumeRole (`role_arn` + `source_profile`), MFA (`mfa_serial`), static credentials (`aws_access_key_id`/`aws_secret_access_key`).

3) Resolve authentication and credentials
- SSO: check state → if not logged in or expired, run `aws sso login --profile <p>` (interactive only).
- MFA + static: prompt for 6-digit code (no echo) → call `aws sts get-session-token`.
- AssumeRole: call `aws sts assume-role` using valid base credentials (MFA step first if required).
- Static-only: use existing static credentials.
- Apply resulting credentials to the child `aws` process environment (execution-scoped only).

4) Execute the command
- Spawn `aws` as a child process with the resolved environment. Forward stdin/stdout/stderr and signals. Exit with the child process status.

### 6.5 Profile resolution algorithm

- Name resolution: when no `--profile` is provided, use `AWS_PROFILE`; otherwise default to `default`.
- Reference integrity: error if `source_profile` is missing or if there is a circular `source_profile` reference (exit code 2).
- Display badges for UX: [SSO], [ROLE], [MFA], [STATIC] (example: `production [SSO][ROLE]`).

### 6.6 SSO detection and handling

- Detection strategy (in order):
    1. Try a lightweight `aws sts get-caller-identity --profile <p>` with a short network timeout.
    2. If the call returns an SSO-related expiry/error, treat the profile as requiring SSO login.
    3. In interactive mode, automatically launch `aws sso login --profile <p>` after informing the user.
- Non-interactive mode: if login is required, fail with exit code 2 and print the exact `aws sso login` command to run.
- Token persistence: `awx` does not save SSO tokens; it relies on AWS CLI's `~/.aws/sso/cache/`.

### 6.7 MFA / STS / AssumeRole processing details

- MFA input: accept only 6-digit numeric codes, no echo, up to 3 retries.
- `get-session-token`: call with `--serial-number <mfa_serial> --token-code <code>`.
- `assume-role`: call with `--role-arn <role_arn> --role-session-name awx-<timestamp>`; ensure the `source_profile` credentials are resolved first (including MFA if required).
- Chaining: MVP supports a single assume-role step; deeper chaining is designated as a future enhancement.
- Respect STS `Expiration` and auto-refresh (interactive only) when expiry is detected.

### 6.8 Environment variables and region resolution

- Set the following environment variables for the child process when appropriate:
    - `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN`
    - `AWS_DEFAULT_REGION` when the profile specifies a region and no explicit region is set by user or command.
- Respect user-provided context: do not override `AWS_REGION`/`AWS_DEFAULT_REGION` or a command-level `--region` option explicitly provided by the user.
- Region precedence:
    1. `--region` argument on the `aws` command
    2. Environment variable `AWS_REGION` or `AWS_DEFAULT_REGION`
    3. Profile `region` setting

### 6.9 Session cache details (opt-in)

- Default behavior: in-memory only; no persistence across process exits.
- Opt-in: enable caching with `AWX_CACHE=1` or a configuration flag.
- Storage priority:
    1. OS keychain via `keyring` (service `awx`, account `<profile>:session`).
    2. Encrypted fallback file: `~/.awx/cache/<profile>.json.enc` with chmod 600.
- Fallback key management:
    - Require a user-supplied passphrase for file fallback to ensure explicit consent.
    - Accept `AWX_CACHE_PASSPHRASE` for non-interactive cache unlocking; otherwise, fallback storage is disabled.
    - Use an authenticated encryption scheme (ring) with random nonce and PBKDF for key derivation.
- Expiration: detect expiry and auto-refresh interactively; expired or corrupted caches are discarded and trigger re-authentication.
- Clear cache: `awx --clear-cache [profile|all]` removes keychain entries and cache files.

### 6.10 Retry and timeout behavior

- STS/AssumeRole calls: retry up to 3 times for transient network errors with exponential backoff (~300ms, ~700ms, ~1500ms + jitter).
- Default timeouts: connect 10s, overall request ~30s; make timeouts configurable via `AWX_CONNECT_TIMEOUT` and `AWX_REQUEST_TIMEOUT`.
- `aws sso login` is user-driven and does not time out by `awx` (user may cancel via browser/terminal).

### 6.11 Signals and exit codes

- Signals: forward SIGINT and SIGTERM to the child process. If the child exits due to signal, `awx` should exit with the corresponding code (e.g., SIGINT → 130).
- Exit codes summary:
    - 0: Success (child process returned 0)
    - 1: Unexpected error (parsing or internal failure)
    - 2: Usage/authentication error where interactive authentication is required but not possible (CI/non-interactive)
    - 3: MFA authentication failure (exhausted retries)
    - 127: `aws` binary not found
    - 130: SIGINT, 143: SIGTERM

### 6.12 Logging and masking

Prefer informative context lines rather than leaking values: e.g. `Calling STS get-session-token (profile=prod, mfa=arn:aws:iam::<ACCOUNT_ID>:mfa/user)`.

### 6.13 Representative scenarios (examples)

1) SSO expired → auto-login → run

```
$ awx -- s3 ls
Select profile: production [SSO]
SSO token is not valid. Running: aws sso login --profile production
SSO login completed. Running command...
> aws s3 ls
2025-10-16  my-bucket
```

2) MFA + AssumeRole

```
$ awx -p prod-admin -- ec2 describe-instances
Enter MFA code (6 digits): ******
Authenticating... STS get-session-token -> OK; assume-role -> OK
> aws ec2 describe-instances (credentials injected via environment)
{ ... output ... }
```

3) CI (non-interactive) with SSO required

```
$ AWX_NO_INTERACTIVE=1 awx -p production -- s3 ls
Error: SSO login required for profile "production". Run: aws sso login --profile production
Exit code: 2
```

### 6.14 Edge cases and behavior

- `source_profile` missing or circular: explicit error (exit code 2).
- Role requires MFA but `mfa_serial` missing: catch assume-role errors and suggest adding `mfa_serial` to the profile or using a base profile that supports MFA.
- Profile names with spaces or special characters: displayed/supported; internal references use raw profile name.
- Piped or redirected sessions (non-TTY): UI falls back to non-interactive mode.
- User cancel during prompt (Esc/Ctrl+C): exit code 130.

## 7. Security policy (required)

- Least persistence: credentials are not persisted by default. SSO tokens are not stored by `awx` (AWS CLI handles SSO caching).
- Sensitive input handling: no-echo for MFA/passphrases; logs must mask secrets.
- Storage: prefer system keychain; fallback encrypted files must be permissioned to owner-only (600) and protected with authenticated encryption.
- In-memory hygiene: clear sensitive buffers where feasible after use.
- Respect external tooling: check for `aws` binary presence and use AWS CLI commands (e.g. `aws configure list-profiles`) safely.

## 8. Error handling (summary)

- `aws` not found: show install guidance and exit 127.
- Profile missing or invalid: show clear instructions (example config) and exit 2.
- SSO missing/expired: prompt (interactive) or instruct CLI command to run (non-interactive).
- MFA invalid/expired: allow up to 3 retries, then exit 3.
- Network/transient errors: exponential backoff with up to 3 attempts.
- Cache corruption: discard and prompt for re-authentication with a warning.

## 9. Technical specification (brief)

- Language: Rust 2021
- Implementation guidance:
    - CLI: `clap` 4.x; interactive elements: `dialoguer` 0.11.x / `console` 0.15.x
    - Config parsing: `ini` 1.x, `dirs` 5.x, `serde`/`serde_json` 1.x, `chrono` 0.4.x
    - Runtime: `tokio` 1.x for async + subprocess management and signal handling
    - Errors: `anyhow` / `thiserror`
    - Secure storage: `keyring` (OS keychain) primary; fallback: `ring` 0.17.x with authenticated encryption
    - Logging: file logs with rotation under `~/.awx/logs/` (mask sensitive fields)
- Profile detection rules (summary): presence of `sso_start_url`/`sso_region` → SSO; presence of `role_arn` → AssumeRole; presence of `mfa_serial` → MFA required.

## 10. Acceptance criteria (MVP)

- Detect and display profiles classified as SSO / AssumeRole / MFA / static.
- Support `--profile` for non-interactive runs. If SSO is expired, advise or launch `aws sso login`.
- For MFA profiles: accept a 6-digit code and obtain session credentials via `get-session-token` and then `assume-role` when required.
- Non-interactive mode must fail immediately with a clear exit code and remediation instructions when interactive authentication is required.
- Session cache is disabled by default; when opt-in, credentials persist in the OS keychain only and expire as expected.
- Preserve child `aws` process exit code and pass through stdin/stdout/stderr.

## 11. Release and roadmap

- Distribution: GitHub Releases and `cargo install` (future: Homebrew formula).
- Future enhancements beyond MVP: multi-hop AssumeRole chains, profile templates, audit logging, integrations for Terraform/Ansible.

## 12. References

- AWS official documentation for AWS CLI config, SSO, and STS. Consider similar tools for reference only: `aws-vault`, `awsume`.

— Prioritize minimal, secure, and predictable UX; delegate SSO to the AWS CLI, and prompt for MFA/AssumeRole only when required —
