//! Integration tests against a real AWS account and Aurora DSQL cluster.
//!
//! Each test is skipped (passes without doing anything) unless the environment variables it needs are set; see
//! `docs/development.md`. Never commit real values.

use dsql_core::{clusters, identity};

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

#[tokio::test]
async fn it_lists_test_cluster() {
    let Some((profile, region)) = aws_env() else {
        eprintln!("skipped: AWS_PROFILE and DSQL_REGION not set");
        return;
    };
    let Ok(host) = std::env::var("DSQL_HOST") else {
        eprintln!("skipped: DSQL_HOST not set");
        return;
    };
    let (id, host_region) = clusters::parse_endpoint(&host).expect("DSQL_HOST is a DSQL endpoint");
    assert_eq!(host_region, region);

    let cfg = identity::sdk_config(&profile, &region)
        .await
        .expect("credentials");
    let started = std::time::Instant::now();
    let (found, failures) = clusters::list_clusters(&cfg, clusters::DSQL_REGIONS).await;
    println!(
        "{} clusters, {} regions skipped, in {} ms",
        found.len(),
        failures.len(),
        started.elapsed().as_millis()
    );
    for (r, e) in &failures {
        println!("skipped {r}: {e}");
    }
    let test = found
        .iter()
        .find(|c| c.identifier == id)
        .expect("test cluster listed");
    assert_eq!(test.region, region);
    assert_eq!(test.endpoint, clusters::endpoint(&id, &region));
    assert_eq!(test.status, "ACTIVE");
    println!(
        "test cluster tags: {:?}",
        test.tags.keys().collect::<Vec<_>>()
    );
}
