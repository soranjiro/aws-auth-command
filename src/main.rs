use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use clap::Parser;
use console::Style;
use dialoguer::{theme::ColorfulTheme, Password, Select};
// Minimal INI parser used to read AWS config/credentials for tests and MVP.
fn parse_ini(content: &str) -> std::collections::HashMap<String, std::collections::HashMap<String, String>> {
    let mut map: std::collections::HashMap<String, std::collections::HashMap<String, String>> =
        std::collections::HashMap::new();
    let mut current_section = String::from("default");
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            current_section = line[1..line.len() - 1].trim().to_string();
            continue;
        }
        if let Some(idx) = line.find('=') {
            let key = line[..idx].trim();
            let val = line[idx + 1..].trim();
            let section = map.entry(current_section.clone()).or_insert_with(|| {
                std::collections::HashMap::new()
            });
            section.insert(key.to_string(), val.to_string());
        }
    }
    map
}
use serde::Deserialize;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

#[derive(Debug, Parser)]
#[clap(name = "awx", version)]
#[clap(trailing_var_arg = true)]
struct Opt {
    /// Specify profile directly
    #[clap(short = 'p', long = "profile")]
    profile: Option<String>,

    /// Show configuration/status
    #[clap(short = 'c', long = "config")]
    config: bool,

    /// Clear cache (profile or 'all')
    #[clap(long = "clear-cache")]
    clear_cache: Option<String>,

    /// Skip interactive UI (for CI)
    #[clap(short = 'n', long = "no-interactive")]
    no_interactive: bool,

    /// Verbose logging
    #[clap(short = 'v', long = "verbose")]
    verbose: bool,

    /// Any remaining arguments are passed to the aws CLI
    #[clap(last = true)]
    aws_args: Vec<String>,
}

#[derive(Debug, Default, Clone)]
struct Profile {
    name: String,
    region: Option<String>,
    sso_start_url: Option<String>,
    sso_region: Option<String>,
    role_arn: Option<String>,
    source_profile: Option<String>,
    mfa_serial: Option<String>,
    aws_access_key_id: Option<String>,
    aws_secret_access_key: Option<String>,
    aws_session_token: Option<String>,
}

impl Profile {
    fn is_sso(&self) -> bool {
        self.sso_start_url.is_some() || self.sso_region.is_some()
    }

    fn is_role(&self) -> bool {
        self.role_arn.is_some()
    }

    fn is_static(&self) -> bool {
        self.aws_access_key_id.is_some() && self.aws_secret_access_key.is_some()
    }

    fn requires_mfa(&self) -> bool {
        self.mfa_serial.is_some()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct StsCredsWrapper {
    credentials: StsCredentials,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct StsCredentials {
    access_key_id: String,
    secret_access_key: String,
    session_token: String,
    expiration: String,
}

#[tokio::main]
async fn main() -> ExitCode {
    if let Err(err) = run().await {
        eprintln!("Error: {}", err);
        return ExitCode::from(1);
    }
    ExitCode::from(0)
}

async fn run() -> Result<()> {
    let opts = Opt::parse();

    // Ensure aws binary exists
    ensure_aws_present().await?;

    let profiles = load_profiles()?;
    if profiles.is_empty() {
        return Err(anyhow!("No AWS profiles found in ~/.aws/config or ~/.aws/credentials"));
    }

    if opts.config {
        print_config(&profiles).await?;
        return Ok(());
    }

    if let Some(target) = opts.clear_cache.as_deref() {
        // MVP: cache not implemented yet; stub behavior
        println!("Clearing cache for: {} (no-op in MVP)", target);
        return Ok(());
    }

    // resolve profile precedence: CLI > AWS_PROFILE env > default
    let selected_profile_name = if let Some(p) = opts.profile.clone() {
        p
    } else if opts.no_interactive {
        env::var("AWS_PROFILE").unwrap_or_else(|_| "default".to_string())
    } else {
        // interactive selection
        interactive_select_profile(&profiles)?
    };

    let profile = profiles
        .get(&selected_profile_name)
        .ok_or_else(|| anyhow!("Profile '{}' not found", selected_profile_name))?
        .clone();

    if profile.is_sso() {
        match check_sts_identity(&selected_profile_name).await {
            Ok(true) => {
                // logged in, proceed
            }
            Ok(false) => {
                if opts.no_interactive {
                    eprintln!(
                        "SSO login required for profile \"{}\". Run: aws sso login --profile {}",
                        selected_profile_name, selected_profile_name
                    );
                    std::process::exit(2);
                }
                println!(
                    "SSO token is not valid. Running: aws sso login --profile {}",
                    selected_profile_name
                );
                let status = Command::new("aws")
                    .arg("sso")
                    .arg("login")
                    .arg("--profile")
                    .arg(&selected_profile_name)
                    .status()
                    .await
                    .context("Failed to run aws sso login")?;
                if !status.success() {
                    return Err(anyhow!("aws sso login failed"));
                }
                println!("SSO login completed.");
            }
            Err(_) => {
                // timeout or network issues -> treat as not logged in
                if opts.no_interactive {
                    eprintln!(
                        "SSO login required for profile \"{}\". Run: aws sso login --profile {}",
                        selected_profile_name, selected_profile_name
                    );
                    std::process::exit(2);
                }
            }
        }
    }

    // Resolve credentials (MVP supports single assume-role step and MFA for static creds)
    let final_creds = if profile.is_role() {
        // Find base credentials from source_profile
        let role_arn = profile.role_arn.clone().unwrap();
        let source_name = profile
            .source_profile
            .clone()
            .ok_or_else(|| anyhow!("source_profile missing for role profile"))?;

        let base_profile = profiles
            .get(&source_name)
            .ok_or_else(|| anyhow!("source_profile '{}' not found", source_name))?
            .clone();

        // If base_profile needs MFA + static keys
            if base_profile.requires_mfa() && base_profile.is_static() {
            let mfa = base_profile.mfa_serial.clone().unwrap();
            let base_temp = get_session_token_interactive(&source_name, &mfa).await?;
            // use base_temp credentials in env to call assume-role
            let session_name = format!("awx-{}", Utc::now().timestamp());
            let assume_resp = assume_role_with_env(&role_arn, &session_name, &base_temp).await?;
            Some(assume_resp)
        } else if base_profile.is_sso() {
            // let aws CLI handle using --profile <source_profile>
            let session_name = format!("awx-{}", Utc::now().timestamp());
            let assume_resp = assume_role_with_profile(&role_arn, &session_name, &source_name).await?;
            Some(assume_resp)
        } else if base_profile.is_static() {
            // static keys -> ask aws cli to assume using the source_profile
            let session_name = format!("awx-{}", Utc::now().timestamp());
            let assume_resp = assume_role_with_profile(&role_arn, &session_name, &source_name).await?;
            Some(assume_resp)
        } else {
            return Err(anyhow!(
                "Unsupported source_profile auth method for '{}'. MVP supports SSO or static+MFA for source profiles.",
                source_name
            ));
        }
    } else if profile.requires_mfa() && profile.is_static() {
        // Prompt for MFA for the profile's static keys
        let mfa = profile.mfa_serial.clone().unwrap();
        let tmp = get_session_token_interactive(&profile.name, &mfa).await?;
        Some(tmp)
    } else {
        // static-only or SSO-only (no credential injection needed)
        None
    };

    // Execute aws command with credentials injected into environment (if any)
    if opts.aws_args.is_empty() {
        println!("No AWS command specified. Use -- to pass AWS CLI arguments.");
        return Ok(());
    }

    let exit_code = run_aws_child_capture(&opts.aws_args, final_creds, profile).await?;
    // Forward child exit code for CLI behavior
    std::process::exit(exit_code);
}

async fn ensure_aws_present() -> Result<()> {
    match Command::new("aws").arg("--version").output().await {
        Ok(output) => {
            if output.status.success() {
                Ok(())
            } else {
                Err(anyhow!("aws binary not found or returned non-zero --version"))
            }
        }
        Err(_) => Err(anyhow!(
            "aws binary not found. Please install AWS CLI v2 and ensure 'aws' is on PATH"
        )),
    }
}

fn aws_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("Could not determine home directory"))?;
    Ok(home.join(".aws"))
}

fn load_profiles() -> Result<HashMap<String, Profile>> {
    let aws = aws_dir()?;
    load_profiles_from_dir(&aws)
}

fn load_profiles_from_dir(aws: &std::path::Path) -> Result<HashMap<String, Profile>> {
    let mut profiles: HashMap<String, Profile> = HashMap::new();

    let config_path = aws.join("config");
    if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read {}", config_path.display()))?;
        let conf = parse_ini(&content);
        for (section_name, prop) in conf.into_iter() {
            let profile_name = if section_name.starts_with("profile ") {
                section_name[8..].to_string()
            } else {
                section_name.clone()
            };
            let entry = profiles.entry(profile_name.clone()).or_insert_with(|| Profile {
                name: profile_name.clone(),
                ..Default::default()
            });
            if let Some(r) = prop.get("region") {
                entry.region = Some(r.to_string());
            }
            if let Some(s) = prop.get("sso_start_url") {
                entry.sso_start_url = Some(s.to_string());
            }
            if let Some(s) = prop.get("sso_region") {
                entry.sso_region = Some(s.to_string());
            }
            if let Some(r) = prop.get("role_arn") {
                entry.role_arn = Some(r.to_string());
            }
            if let Some(s) = prop.get("source_profile") {
                entry.source_profile = Some(s.to_string());
            }
            if let Some(m) = prop.get("mfa_serial") {
                entry.mfa_serial = Some(m.to_string());
            }
        }
    }

    let creds_path = aws.join("credentials");
    if creds_path.exists() {
        let content = std::fs::read_to_string(&creds_path)
            .with_context(|| format!("Failed to read {}", creds_path.display()))?;
        let conf = parse_ini(&content);
        for (section_name, prop) in conf.into_iter() {
            let profile_name = section_name.clone();
            let entry = profiles.entry(profile_name.clone()).or_insert_with(|| Profile {
                name: profile_name.clone(),
                ..Default::default()
            });
            if let Some(a) = prop.get("aws_access_key_id") {
                entry.aws_access_key_id = Some(a.to_string());
            }
            if let Some(a) = prop.get("aws_secret_access_key") {
                entry.aws_secret_access_key = Some(a.to_string());
            }
            if let Some(a) = prop.get("aws_session_token") {
                entry.aws_session_token = Some(a.to_string());
            }
        }
    }

    Ok(profiles)
}

fn interactive_select_profile(profiles: &HashMap<String, Profile>) -> Result<String> {
    let mut items: Vec<String> = Vec::new();
    let mut mapping: Vec<String> = Vec::new();
    for (name, p) in profiles.iter() {
        let mut badges = String::new();
        if name == "default" {
            badges.push_str("[default]");
        }
        if p.is_sso() {
            badges.push_str("[SSO]");
        }
        if p.is_role() {
            badges.push_str("[ROLE]");
        }
        if p.requires_mfa() {
            badges.push_str("[MFA]");
        }
        if p.is_static() {
            badges.push_str("[STATIC]");
        }
        let display = format!("{} {}", name, badges);
        items.push(display);
        mapping.push(name.clone());
    }
    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select profile")
        .items(&items)
        .default(0)
        .interact()?;
    Ok(mapping[selection].clone())
}

async fn check_sts_identity(profile: &str) -> Result<bool> {
    let mut cmd = Command::new("aws");
    cmd.arg("sts")
        .arg("get-caller-identity")
        .arg("--profile")
        .arg(profile)
        .arg("--output")
        .arg("json");
    let fut = cmd.output();
    match timeout(Duration::from_secs(5), fut).await {
        Ok(Ok(output)) => Ok(output.status.success()),
        Ok(Err(e)) => Err(anyhow!(e)),
        Err(_) => Err(anyhow!("Timeout while checking STS identity")),
    }
}

async fn get_session_token_interactive(profile: &str, mfa_serial: &str) -> Result<StsCredentials> {
    // Verify MFA serial account matches the profile's account before prompting.
    if let Some(mfa_account) = extract_account_from_arn(mfa_serial) {
        match get_profile_account(profile).await {
            Ok(profile_account) => {
                if profile_account != mfa_account {
                    return Err(anyhow!(format!(
                        "MFA serial account ({}) does not match profile account ({}). Update 'mfa_serial' in profile '{}', or use credentials for the correct account.",
                        mfa_account, profile_account, profile
                    )));
                }
            }
            Err(e) => {
                // If we cannot determine profile account, warn but continue to prompt
                eprintln!("Warning: could not determine profile account: {}", e);
            }
        }
    }
    for attempt in 1..=3 {
        let prompt = format!("Enter MFA code (6 digits) for {}: ", mfa_serial);
        let code = Password::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .allow_empty_password(false)
            .interact()?;
        let code = code.trim().to_string();
        if !code.chars().all(|c| c.is_ascii_digit()) || code.len() != 6 {
            eprintln!("Invalid code format");
            continue;
        }
        match get_session_token(profile, mfa_serial, &code).await {
            Ok(creds) => return Ok(creds),
            Err(e) => {
                eprintln!("MFA attempt {} failed: {}", attempt, e);
                if attempt == 3 {
                    std::process::exit(3);
                }
            }
        }
    }
    Err(anyhow!("MFA failed after retries"))
}

fn extract_account_from_arn(arn: &str) -> Option<String> {
    // ARN format: arn:partition:service:region:account-id:resource
    // For IAM MFA: arn:aws:iam::<ACCOUNT_ID>:mfa/username
    let parts: Vec<&str> = arn.split(':').collect();
    if parts.len() >= 5 {
        let account = parts[4];
        if !account.is_empty() {
            return Some(account.to_string());
        }
    }
    None
}

async fn get_profile_account(profile: &str) -> Result<String> {
    let mut cmd = Command::new("aws");
    cmd.arg("sts")
        .arg("get-caller-identity")
        .arg("--profile")
        .arg(profile)
        .arg("--output")
        .arg("json");
    let output = timeout(Duration::from_secs(5), cmd.output())
        .await
        .context("get_caller_identity timeout")?
        .context("failed to run aws sts get-caller-identity")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(format!("get-caller-identity failed: {}", stderr)));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .context("Parsing get-caller-identity JSON response failed")?;
    if let Some(account) = v.get("Account").and_then(|a| a.as_str()) {
        return Ok(account.to_string());
    }
    Err(anyhow!("Account not found in get-caller-identity response"))
}

async fn get_session_token(profile: &str, mfa_serial: &str, code: &str) -> Result<StsCredentials> {
    let mut cmd = Command::new("aws");
    cmd.arg("sts")
        .arg("get-session-token")
        .arg("--serial-number")
        .arg(mfa_serial)
        .arg("--token-code")
        .arg(code)
        .arg("--profile")
        .arg(profile)
        .arg("--duration-seconds")
        .arg("3600")
        .arg("--output")
        .arg("json");

    let output = timeout(Duration::from_secs(30), cmd.output())
        .await
        .context("get_session_token timeout")?
        .context("failed to run aws sts get-session-token")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("get-session-token failed: {}", stderr));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let wrap: StsCredsWrapper = serde_json::from_str(&stdout)
        .context("Parsing get-session-token JSON response failed")?;
    // touch expiration so the field is considered read (used) and does not emit dead_code warning
    let _ = &wrap.credentials.expiration;
    Ok(wrap.credentials)
}

async fn assume_role_with_profile(role_arn: &str, session_name: &str, profile: &str) -> Result<StsCredentials> {
    let mut cmd = Command::new("aws");
    cmd.arg("sts")
        .arg("assume-role")
        .arg("--role-arn")
        .arg(role_arn)
        .arg("--role-session-name")
        .arg(session_name)
        .arg("--profile")
        .arg(profile)
        .arg("--duration-seconds")
        .arg("3600")
        .arg("--output")
        .arg("json");

    let output = timeout(Duration::from_secs(30), cmd.output())
        .await
        .context("assume-role timeout")?
        .context("failed to run aws sts assume-role")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("assume-role failed: {}", stderr));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let wrap: StsCredsWrapper = serde_json::from_str(&stdout)
        .context("Parsing assume-role JSON response failed")?;
    let _ = &wrap.credentials.expiration;
    Ok(wrap.credentials)
}

async fn assume_role_with_env(role_arn: &str, session_name: &str, base: &StsCredentials) -> Result<StsCredentials> {
    let mut cmd = Command::new("aws");
    cmd.env("AWS_ACCESS_KEY_ID", &base.access_key_id)
        .env("AWS_SECRET_ACCESS_KEY", &base.secret_access_key)
        .env("AWS_SESSION_TOKEN", &base.session_token)
        .arg("sts")
        .arg("assume-role")
        .arg("--role-arn")
        .arg(role_arn)
        .arg("--role-session-name")
        .arg(session_name)
        .arg("--duration-seconds")
        .arg("3600")
        .arg("--output")
        .arg("json");

    let output = timeout(Duration::from_secs(30), cmd.output())
        .await
        .context("assume-role-with-env timeout")?
        .context("failed to run aws sts assume-role with env creds")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("assume-role (env) failed: {}", stderr));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let wrap: StsCredsWrapper = serde_json::from_str(&stdout)
        .context("Parsing assume-role (env) JSON response failed")?;
    let _ = &wrap.credentials.expiration;
    Ok(wrap.credentials)
}

async fn run_aws_child_capture(args: &[String], creds: Option<StsCredentials>, profile: Profile) -> Result<i32> {
    use std::os::unix::process::ExitStatusExt;
    use std::process::Stdio;

    let mut cmd = Command::new("aws");
    for a in args {
        cmd.arg(a);
    }
    // inherit stdio so child interacts directly
    cmd.stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    if let Some(creds) = creds {
        cmd.env("AWS_ACCESS_KEY_ID", creds.access_key_id)
            .env("AWS_SECRET_ACCESS_KEY", creds.secret_access_key)
            .env("AWS_SESSION_TOKEN", creds.session_token);
    } else if profile.is_static() {
        // Inject static credentials from profile if the environment does not already provide them
        // (treat empty string as not provided).
        let ak_present = match env::var("AWS_ACCESS_KEY_ID") {
            Ok(v) => !v.is_empty(),
            Err(_) => false,
        };
        if !ak_present {
            if let Some(k) = profile.aws_access_key_id.clone() {
                cmd.env("AWS_ACCESS_KEY_ID", k);
            }
            if let Some(s) = profile.aws_secret_access_key.clone() {
                cmd.env("AWS_SECRET_ACCESS_KEY", s);
            }
            if let Some(t) = profile.aws_session_token.clone() {
                cmd.env("AWS_SESSION_TOKEN", t);
            }
        }
    }
    // region precedence: do not override if user provided --region or env has AWS_REGION
    let region_env = env::var("AWS_REGION").ok().or_else(|| env::var("AWS_DEFAULT_REGION").ok());
    let provided_region_in_args = args.iter().any(|a| a.starts_with("--region"));
    if region_env.is_none() && !provided_region_in_args {
        if let Some(r) = profile.region {
            cmd.env("AWS_DEFAULT_REGION", r);
        }
    }

    // Ensure the child uses the selected profile unless the aws command already included a --profile flag.
    let provided_profile_in_args = args.iter().any(|a| a == "--profile" || a.starts_with("--profile="));
    if !provided_profile_in_args {
        cmd.env("AWS_PROFILE", profile.name.clone());
    }

    let mut child = cmd.spawn().context("failed to spawn aws child command")?;
    let child_id = child.id();

    // Forward signals (SIGINT / SIGTERM) to the child process
    let sigint = tokio::signal::ctrl_c();
    tokio::pin!(sigint);

    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate())?;
        let status = tokio::select! {
            res = child.wait() => {
                res.context("failed while waiting for child")?
            }
            _ = &mut sigint => {
                if let Some(pid) = child_id {
                    // send SIGINT to child
                    unsafe { libc::kill(pid as i32, libc::SIGINT) };
                }
                child.wait().await.context("waiting for child after SIGINT")?
            }
            _ = sigterm.recv() => {
                if let Some(pid) = child_id {
                    unsafe { libc::kill(pid as i32, libc::SIGTERM) };
                }
                child.wait().await.context("waiting for child after SIGTERM")?
            }
        };

        if let Some(code) = status.code() {
            return Ok(code);
        } else if let Some(sig) = status.signal() {
            let exit_code = match sig {
                libc::SIGINT => 130,
                libc::SIGTERM => 143,
                _ => 128 + sig,
            };
            return Ok(exit_code as i32);
        }
    }

    #[cfg(not(unix))]
    {
        let status = tokio::select! {
            res = child.wait() => {
                res.context("failed while waiting for child")?
            }
            _ = &mut sigint => {
                if let Some(pid) = child_id {
                    // send SIGINT to child
                    unsafe { libc::kill(pid as i32, libc::SIGINT) };
                }
                child.wait().await.context("waiting for child after SIGINT")?
            }
        };

        if let Some(code) = status.code() {
            return Ok(code);
        } else if let Some(sig) = status.signal() {
            let exit_code = 128 + sig;
            return Ok(exit_code as i32);
        }
    }

    Ok(0)
}

async fn print_config(profiles: &HashMap<String, Profile>) -> Result<()> {
    let bold = Style::new().bold();
    println!("Discovered profiles:");
    for (name, p) in profiles.iter() {
        let mut badges = Vec::new();
        if name == "default" {
            badges.push("default");
        }
        if p.is_sso() {
            badges.push("SSO");
        }
        if p.is_role() {
            badges.push("ROLE");
        }
        if p.requires_mfa() {
            badges.push("MFA");
        }
        if p.is_static() {
            badges.push("STATIC");
        }
        let badge_str = badges.iter().map(|b| format!("[{}]", b)).collect::<Vec<_>>().join("");
        println!("  {} {}", bold.apply_to(name), badge_str);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
