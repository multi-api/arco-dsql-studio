//! Finding a profile's Aurora DSQL clusters across regions.

use std::collections::BTreeMap;

use aws_config::{Region, SdkConfig};
use serde::Serialize;

use crate::error::CoreError;
use crate::identity::aws_error;

/// Regions where Aurora DSQL is available, from AWS's public SSM parameters
/// (`/aws/service/global-infrastructure/services/dsql/regions`) on 2026-11-05. Keep this the only list.
pub const DSQL_REGIONS: &[&str] = &[
    "ap-east-1",
    "ap-northeast-1",
    "ap-northeast-2",
    "ap-northeast-3",
    "ap-south-1",
    "ap-southeast-1",
    "ap-southeast-2",
    "ap-southeast-4",
    "ca-central-1",
    "ca-west-1",
    "eu-central-1",
    "eu-north-1",
    "eu-south-2",
    "eu-west-1",
    "eu-west-2",
    "eu-west-3",
    "sa-east-1",
    "us-east-1",
    "us-east-2",
    "us-west-2",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClusterInfo {
    pub identifier: String,
    pub region: String,
    pub status: String,
    pub endpoint: String,
    pub arn: String,
    pub tags: BTreeMap<String, String>,
}

/// The cluster's Postgres endpoint: `<id>.dsql.<region>.on.aws`.
pub fn endpoint(identifier: &str, region: &str) -> String {
    format!("{identifier}.dsql.{region}.on.aws")
}

/// `(identifier, region)` from a DSQL endpoint, for clusters entered by hand. `None` for any other host.
pub fn parse_endpoint(host: &str) -> Option<(String, String)> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    let rest = host.strip_suffix(".on.aws")?;
    let (identifier, region) = rest.split_once(".dsql.")?;
    let identifier_ok =
        !identifier.is_empty() && identifier.chars().all(|c| c.is_ascii_alphanumeric());
    identifier_ok.then_some(())?;
    is_region_name(region).then(|| (identifier.to_string(), region.to_string()))
}

/// `xx-name-N` shapes such as `us-east-1` or `ap-southeast-4`.
fn is_region_name(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() >= 3
        && parts[0].len() == 2
        && parts[..parts.len() - 1]
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_lowercase()))
        && parts[parts.len() - 1].chars().all(|c| c.is_ascii_digit())
        && !parts[parts.len() - 1].is_empty()
}

/// Lists clusters in every region at once. A region that fails (no access, opt-in region not enabled, DSQL missing)
/// goes into the second vector and never fails the whole call. Clusters come back sorted by region, then identifier.
pub async fn list_clusters(
    base: &SdkConfig,
    regions: &[&str],
) -> (Vec<ClusterInfo>, Vec<(String, CoreError)>) {
    let mut tasks = tokio::task::JoinSet::new();
    for region in regions {
        let cfg = base
            .to_builder()
            .region(Region::new(region.to_string()))
            .build();
        let region = region.to_string();
        tasks.spawn(async move {
            let result = list_region(&cfg, &region).await;
            (region, result)
        });
    }

    let mut clusters = Vec::new();
    let mut failures = Vec::new();
    while let Some(joined) = tasks.join_next().await {
        match joined {
            Ok((_, Ok(found))) => clusters.extend(found),
            Ok((region, Err(e))) => failures.push((region, e)),
            Err(e) => failures.push((
                "?".to_string(),
                CoreError::Io(format!("region task failed: {e}")),
            )),
        }
    }
    clusters.sort_by(|a, b| (&a.region, &a.identifier).cmp(&(&b.region, &b.identifier)));
    failures.sort_by(|a, b| a.0.cmp(&b.0));
    (clusters, failures)
}

/// `ListClusters` (paginated), then `GetCluster` for each one: it returns the status, endpoint and tags together.
async fn list_region(cfg: &SdkConfig, region: &str) -> Result<Vec<ClusterInfo>, CoreError> {
    let client = aws_sdk_dsql::Client::new(cfg);
    let mut identifiers = Vec::new();
    let mut next = None;
    loop {
        let page = client
            .list_clusters()
            .set_next_token(next)
            .send()
            .await
            .map_err(aws_error)?;
        identifiers.extend(page.clusters().iter().map(|c| c.identifier().to_string()));
        next = page.next_token().map(str::to_string);
        if next.is_none() {
            break;
        }
    }

    let mut out = Vec::with_capacity(identifiers.len());
    for id in identifiers {
        let c = client
            .get_cluster()
            .identifier(&id)
            .send()
            .await
            .map_err(aws_error)?;
        out.push(ClusterInfo {
            identifier: c.identifier().to_string(),
            region: region.to_string(),
            status: c.status().as_str().to_string(),
            endpoint: c
                .endpoint()
                .map(str::to_string)
                .unwrap_or_else(|| endpoint(c.identifier(), region)),
            arn: c.arn().to_string(),
            tags: c
                .tags()
                .map(|t| t.clone().into_iter().collect())
                .unwrap_or_default(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_format() {
        assert_eq!(
            endpoint("abcdefghij0123456789abcdef", "us-east-1"),
            "abcdefghij0123456789abcdef.dsql.us-east-1.on.aws"
        );
    }

    #[test]
    fn parse_endpoint_roundtrip() {
        let host = endpoint("abcdefghij0123456789abcdef", "eu-west-2");
        assert_eq!(
            parse_endpoint(&host),
            Some(("abcdefghij0123456789abcdef".into(), "eu-west-2".into()))
        );
        // Pasted with capitals, spaces or a trailing dot.
        assert_eq!(
            parse_endpoint("  ABCDEFGHIJ0123456789ABCDEF.dsql.ap-southeast-4.on.aws. "),
            Some(("abcdefghij0123456789abcdef".into(), "ap-southeast-4".into()))
        );
    }

    #[test]
    fn parse_endpoint_rejects_non_dsql_host() {
        for host in [
            "",
            "mydb.cluster-abc.us-east-1.rds.amazonaws.com",
            "abc.dsql.us-east-1.on.aws.evil.com",
            ".dsql.us-east-1.on.aws",
            "abc.dsql..on.aws",
            "abc.dsql.not a region.on.aws",
            "a.b.dsql.us-east-1.on.aws",
            "localhost",
        ] {
            assert_eq!(parse_endpoint(host), None, "{host:?}");
        }
    }

    #[test]
    fn regions_are_sorted_unique_and_include_us_east_1() {
        let mut sorted = DSQL_REGIONS.to_vec();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted, DSQL_REGIONS);
        assert!(DSQL_REGIONS.contains(&"us-east-1"));
        assert_eq!(DSQL_REGIONS.len(), 20);
    }
}
