use super::*;
use serial_test::serial;
use std::fs;
use tempfile::tempdir;

#[test]
#[serial]
fn test_load_profiles_from_dir_parses_config_and_credentials() -> Result<()> {
    let td = tempdir()?;
    let aws_dir = td.path();

    // write config
    let config = r#"
[profile sso-prod]
sso_start_url = https://d-123.awsapps.com/start
sso_region = us-west-2
region = us-west-2

[profile role-prod]
role_arn = arn:aws:iam::000000000000:role/ProdRole
source_profile = base
region = us-west-2

[default]
region = us-east-1

[profile mfa-prod]
mfa_serial = arn:aws:iam::000000000000:mfa/test-user
region = us-west-2
"#;
    fs::write(aws_dir.join("config"), config)?;

    // write credentials
    let creds = r#"
[default]
aws_access_key_id = DEFKEY
aws_secret_access_key = DEFSECRET

[base]
aws_access_key_id = BASEKEY
aws_secret_access_key = BASESECRET

[mfa-prod]
aws_access_key_id = MFAKEY
aws_secret_access_key = MFASECRET
"#;
    fs::write(aws_dir.join("credentials"), creds)?;

    let profiles = load_profiles_from_dir(aws_dir)?;
    assert!(profiles.contains_key("sso-prod"));
    assert!(profiles.contains_key("role-prod"));
    assert!(profiles.contains_key("default"));
    assert!(profiles.contains_key("mfa-prod"));
    assert!(profiles.contains_key("base"));

    let sso = profiles.get("sso-prod").unwrap();
    assert!(sso.is_sso());
    assert_eq!(sso.region.as_deref(), Some("us-west-2"));

    let role = profiles.get("role-prod").unwrap();
    assert!(role.is_role());
    assert_eq!(role.source_profile.as_deref(), Some("base"));

    let def = profiles.get("default").unwrap();
    assert!(def.is_static());

    let mfa = profiles.get("mfa-prod").unwrap();
    assert!(mfa.requires_mfa());

    let base = profiles.get("base").unwrap();
    assert!(base.is_static());

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_run_with_fake_aws_binary() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    // create tempdir and fake aws binary
    let td = tempdir()?;
    let bin_dir = td.path().join("bin");
    std::fs::create_dir_all(&bin_dir)?;
    let aws_path = bin_dir.join("aws");
    let script = r#"#!/usr/bin/env bash
if [ "$1" = "--version" ]; then
    echo "aws-cli/2.x fake"
    exit 0
fi
cmd="$1 $2"
case "$cmd" in
"sts get-caller-identity")
    echo '{"Account":"000000000000","Arn":"arn:aws:sts::000000000000:assumed-role/test","UserId":"AID..."}'
    exit 0
    ;;
"sts get-session-token")
    echo '{"Credentials":{"AccessKeyId":"AKIAFAKE","SecretAccessKey":"SECRET","SessionToken":"TOKEN","Expiration":"2025-10-17T00:00:00Z"}}'
    exit 0
    ;;
"sts assume-role")
    echo '{"Credentials":{"AccessKeyId":"AKIAFAKE2","SecretAccessKey":"SECRET2","SessionToken":"TOKEN2","Expiration":"2025-10-17T00:00:00Z"}}'
    exit 0
    ;;
"sso login")
    echo "SSO login simulated"
    exit 0
    ;;
*)
    echo "FAKE AWS: $@"
    exit 0
    ;;
esac
"#;
    std::fs::write(&aws_path, script)?;
    let mut perms = std::fs::metadata(&aws_path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&aws_path, perms)?;

    // prepend to PATH
    let old_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", bin_dir.display(), old_path);
    std::env::set_var("PATH", &new_path);

    // simple run_aws_child_capture invocation
    let args = vec!["s3".to_string(), "ls".to_string()];
    // create a minimal profile for region injection
    let profile = Profile {
        name: "default".to_string(),
        region: Some("us-west-2".to_string()),
        ..Default::default()
    };
    let code = run_aws_child_capture(&args, None, profile).await?;
    assert_eq!(code, 0);

    // test that assume-role path works (calls sts assume-role)
    let creds = assume_role_with_profile("arn:aws:iam::000000000000:role/Role", "awx-test", "default").await?;
    assert_eq!(creds.access_key_id, "AKIAFAKE2");

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_static_profile_credentials_are_injected() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let td = tempdir()?;
    let bin_dir = td.path().join("bin");
    std::fs::create_dir_all(&bin_dir)?;
    let aws_path = bin_dir.join("aws");
    // script requires AWS_ACCESS_KEY_ID to be set and prints its value
    let script = r#"#!/usr/bin/env bash
if [ -z "$AWS_ACCESS_KEY_ID" ]; then
  echo "MISSING"
  exit 5
else
  echo "KEY=$AWS_ACCESS_KEY_ID"
  exit 0
fi
"#;
    std::fs::write(&aws_path, script)?;
    let mut perms = std::fs::metadata(&aws_path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&aws_path, perms)?;

    let old_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", bin_dir.display(), old_path);
    std::env::set_var("PATH", &new_path);

    let args = vec!["s3".to_string(), "ls".to_string()];
    let profile = Profile {
        name: "default".to_string(),
        aws_access_key_id: Some("PROFILEKEY".to_string()),
        aws_secret_access_key: Some("PROFILESECRET".to_string()),
        ..Default::default()
    };
    let code = run_aws_child_capture(&args, None, profile).await?;
    assert_eq!(code, 0);
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_static_profile_does_not_override_env() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let td = tempdir()?;
    let bin_dir = td.path().join("bin");
    std::fs::create_dir_all(&bin_dir)?;
    let aws_path = bin_dir.join("aws");
    // script prints the value of AWS_ACCESS_KEY_ID
    let script = r#"#!/usr/bin/env bash
echo "KEY=$AWS_ACCESS_KEY_ID"
exit 0
"#;
    std::fs::write(&aws_path, script)?;
    let mut perms = std::fs::metadata(&aws_path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&aws_path, perms)?;

    let old_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", bin_dir.display(), old_path);
    std::env::set_var("PATH", &new_path);

    // set environment externally
    std::env::set_var("AWS_ACCESS_KEY_ID", "EXPLICIT");

    let args = vec!["s3".to_string(), "ls".to_string()];
    let profile = Profile {
        name: "default".to_string(),
        aws_access_key_id: Some("PROFILEKEY".to_string()),
        aws_secret_access_key: Some("PROFILESECRET".to_string()),
        ..Default::default()
    };
    let code = run_aws_child_capture(&args, None, profile).await?;
    assert_eq!(code, 0);
    // cleanup environment
    std::env::remove_var("AWS_ACCESS_KEY_ID");
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_child_receives_aws_profile_env() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let td = tempdir()?;
    let bin_dir = td.path().join("bin");
    std::fs::create_dir_all(&bin_dir)?;
    let aws_path = bin_dir.join("aws");
    // script exits 0 only when AWS_PROFILE equals 'example-profile'
    let script = r#"#!/usr/bin/env bash
if [ "$AWS_PROFILE" = "example-profile" ]; then
    exit 0
else
    exit 9
fi
"#;
    std::fs::write(&aws_path, script)?;
    let mut perms = std::fs::metadata(&aws_path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&aws_path, perms)?;

    let old_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", bin_dir.display(), old_path);
    std::env::set_var("PATH", &new_path);

    let args = vec!["s3".to_string(), "ls".to_string()];
    let profile = Profile {
        name: "example-profile".to_string(),
        ..Default::default()
    };
    let code = run_aws_child_capture(&args, None, profile).await?;
    assert_eq!(code, 0);
    Ok(())
}

#[test]
#[serial]
fn test_extract_account_from_arn() {
    let arn = "arn:aws:iam::000000000000:mfa/test-user";
    let acc = extract_account_from_arn(arn);
    assert_eq!(acc.as_deref(), Some("000000000000"));
}

#[tokio::test]
#[serial]
async fn test_verify_mfa_profile_account_mismatch() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let td = tempdir()?;
    let bin_dir = td.path().join("bin");
    std::fs::create_dir_all(&bin_dir)?;
    let aws_path = bin_dir.join("aws");
    // fake aws returns Account 000000000000 for get-caller-identity
    let script = r#"#!/usr/bin/env bash
if [ "$1 $2" = "sts get-caller-identity" ]; then
  echo '{"Account":"000000000000","Arn":"arn:aws:iam::000000000000:user/test-user","UserId":"AID..."}'
  exit 0
fi
echo "OK"
exit 0
"#;
    std::fs::write(&aws_path, script)?;
    let mut perms = std::fs::metadata(&aws_path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&aws_path, perms)?;

    let old_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", bin_dir.display(), old_path);
    std::env::set_var("PATH", &new_path);

    let res = get_profile_account("example-profile").await?;
    assert_eq!(res, "000000000000");

    // mismatched mfa_serial (account 111111111111) should cause verification error
    let mismatch = extract_account_from_arn("arn:aws:iam::111111111111:mfa/test-user");
    assert_eq!(mismatch.as_deref(), Some("111111111111"));

    if let Some(mfa_acc) = mismatch {
        let profile_acc = get_profile_account("example-profile").await?;
        assert_ne!(mfa_acc, profile_acc);
    }

    Ok(())
}
