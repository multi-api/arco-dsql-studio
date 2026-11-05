# Connecting

How Arco DSQL Studio finds your AWS identity and your Aurora DSQL clusters, and what it needs from AWS.

## AWS profiles

On launch the app reads your AWS profiles from `~/.aws/config` and `~/.aws/credentials` (or the files named by
`AWS_CONFIG_FILE` and `AWS_SHARED_CREDENTIALS_FILE`) and lists them with their kind:

| Badge | Profile |
| --- | --- |
| SSO | IAM Identity Center (`sso_session` or the legacy `sso_start_url` in the profile) |
| Access keys | `aws_access_key_id` in the credentials file |
| Role | `role_arn` with a `source_profile` |
| Process | `credential_process` |

The app uses these profiles exactly as the AWS CLI does. It never reads, shows or stores your secrets.

**Sign in** (SSO profiles only) does what `aws sso login --profile <name>` does: it opens your browser at the AWS
access portal, shows the code the browser should display, waits for you to approve, and saves the session in the AWS
CLI's SSO cache (`~/.aws/sso/cache/`). The AWS CLI and other tools see the session too.

## Set up AWS access

For a computer with no AWS profile, **Set up AWS access** creates an SSO profile:

1. Enter your organisation's **AWS access portal URL** (it looks like `https://your-org.awsapps.com/start`; it is in
   the IAM Identity Center invitation email) and its **SSO region**.
2. Approve the sign-in in the browser; check that the code matches.
3. Pick the account and role to use.
4. Choose the profile name and default region.
5. The app shows the **exact lines** it will add to `~/.aws/config`, for example:

   ```ini
   [profile staging-developer]
   sso_session = your-org
   sso_account_id = 111122223333
   sso_role_name = Developer
   region = us-east-1

   [sso-session your-org]
   sso_start_url = https://your-org.awsapps.com/start
   sso_region = us-east-1
   sso_registration_scopes = sso:account:access
   ```

   Nothing is written until you click **Add to ~/.aws/config**. The lines go at the end of the file; nothing already
   there is changed or removed. A profile or `sso-session` with the same name is refused rather than overwritten.

The wizard does not set up access keys: the app never handles secrets. Profiles with access keys that already exist
can be used.

## Clusters

After you pick a profile the app shows who you are ("Signed in as `<role>` in account `<id>`") and lists your DSQL
clusters, grouped by region, with their status and tags (`Name` is used as the cluster's title).

- The first time, every region where DSQL exists is scanned in parallel. Regions where your profile has no access, or
  that are not enabled for your account, are skipped quietly; other failures are listed as "Could not check …".
- The app remembers the regions of the clusters you connect to and scans only those next time. **Scan all regions**
  looks everywhere again.
- **Or enter an endpoint** connects to a cluster you cannot list (for example without `dsql:ListClusters`): paste
  `<cluster-id>.dsql.<region>.on.aws`.

## Connect as admin or a custom role

| Connect as | What it is | AWS permission | In the database |
| --- | --- | --- | --- |
| **admin** | The cluster's built-in superuser. Sees every schema, but cannot read an app's tables until their owner grants it. | `dsql:DbConnectAdmin` on the cluster | — |
| **A custom role** (for example `readonly`) | A Postgres role mapped to your IAM role. Sees what was granted to it. | `dsql:DbConnect` on the cluster | `AWS IAM GRANT <role> TO '<your IAM role ARN>'` |

The app signs in with a short-lived IAM token generated from your profile (nothing to type, nothing stored). If DSQL
refuses, the app explains which of the above is missing, with your IAM role's ARN.

**Read-only** (chosen before connecting) makes the app refuse any statement that writes or changes the schema, before
sending it.

## Schema

After connecting, the app picks the schema your queries run in (`SET search_path`), so `select * from users` works:

1. the role's own default `search_path`, if it names an existing schema;
2. otherwise the only schema other than `public`;
3. otherwise a schema named like the role.

If none of these is clear, it asks you to pick one. You can switch schema later.

## Saving the connection

**Save and open** stores the connection (profile, region, cluster, role, schema, production flag, read-only flag) in
the app's local database, so it appears under **Your connections** for one-click reconnect. No password, token or key
is stored.

**This is production** is ticked for you when the schema ends in `_production` or `_prod`, or a cluster tag says
`production`; change it if that is wrong. On a production connection the app asks for confirmation before anything
that writes or changes the schema.

## IAM permissions

| Action | Needed for |
| --- | --- |
| `sts:GetCallerIdentity` | "Signed in as …" (always allowed) |
| `dsql:ListClusters`, `dsql:GetCluster` | Listing clusters and their tags (optional: you can enter an endpoint) |
| `dsql:DbConnectAdmin` | Connecting as `admin` |
| `dsql:DbConnect` | Connecting as a custom role |
| `iam:GetRole` | Showing your IAM role's exact ARN in explanations and the role wizard (optional) |
