//! Integration tests against a real AWS account and Aurora DSQL cluster.
//!
//! Each test is skipped (passes without doing anything) unless the environment variables it needs are set; see
//! `docs/development.md`. Never commit real values.

use dsql_core::identity;

/// `AWS_PROFILE` and `DSQL_REGION`, or `None` (test skipped) when either is unset.
fn aws_env() -> Option<(String, String)> {
    let profile = std::env::var("AWS_PROFILE")
        .ok()
        .filter(|v| !v.is_empty())?;
    let region = std::env::var("DSQL_REGION")
        .ok()
        .filter(|v| !v.is_empty())?;
    Some((profile, region))
}

#[tokio::test]
async fn it_caller_identity() {
    let Some((profile, region)) = aws_env() else {
        eprintln!("skipped: AWS_PROFILE and DSQL_REGION not set");
        return;
    };
    let cfg = identity::sdk_config(&profile, &region)
        .await
        .expect("credentials");
    let me = identity::caller_identity(&cfg)
        .await
        .expect("caller identity");
    println!("account {} arn {}", me.account, me.arn);
    assert_eq!(me.account.len(), 12);

    // The IAM role behind an SSO session: GetRole when allowed, else the documented path pattern.
    if let Some(role) = identity::assumed_role_name(&me.arn) {
        match identity::iam_role_arn(&cfg, &role).await {
            Ok(arn) => {
                println!("iam role {arn}");
                assert!(arn.ends_with(&format!("/{role}")));
            }
            Err(e) => println!(
                "iam:GetRole not allowed ({e}); the app would fall back to sso_role_arn_guess"
            ),
        }
    }
}
