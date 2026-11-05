# Team access

How to let teammates query an app's tables with their own IAM role, check what a role can do, and remove a role
cleanly.

## Why this is needed

In Aurora DSQL an app's tables belong to the app's own Postgres role (in Arco Cloud apps, the role the app's Lambda
signs in as). Nobody can sign in as that role from outside, and even `admin` cannot read its tables until the owner
grants it. So to query the data yourself you need a role of your own, mapped to your IAM role, with the owner's
grants.

## Give the team access

Connected as **admin**, pick the schema, then **Access… → Give the team access**:

1. **Read only** (`SELECT`) or **read and write** (`SELECT, INSERT, UPDATE, DELETE`) on every table of the schema.
2. **The IAM role to map.** It defaults to the IAM role behind your profile, with its full path (for SSO roles,
   `aws-reserved/sso.amazonaws.com/...`). If your profile may not call `iam:GetRole`, the app builds the ARN from the
   SSO pattern and asks you to check it. Use an IAM role ARN (`arn:aws:iam::…:role/…`), never the `sts::…:assumed-role`
   one.
3. **The role name**, for example `readonly` or `team_rw`.
4. **Review.** The app shows every statement before running anything:

   As `admin`, run here:

   ```sql
   CREATE ROLE "readonly" WITH LOGIN;
   AWS IAM GRANT "readonly" TO 'arn:aws:iam::<account>:role/<path>/<name>';
   GRANT USAGE ON SCHEMA "<schema>" TO "readonly";
   ALTER ROLE "readonly" SET search_path = "<schema>";
   ```

   As the tables' owner, one per table:

   ```sql
   GRANT SELECT ON "<schema>"."<table>" TO "readonly";
   ```

   **Run the admin steps** runs the first block, one statement at a time (DSQL allows one DDL statement per
   transaction).
5. **The owner's grants.** Pick one way:
   - **Run it for me** (Arco Cloud apps): the app invokes the environment's console Lambda,
     `<app>-<environment>-console` (alias `live`), with your own AWS profile and the tinker code it shows, then shows
     the output. The function name is guessed from the schema (`shop_staging` → `shop-staging-console`); correct it
     if needed.
   - **Copy and run it yourself**: the same grants as a tinker snippet (for Arco Cloud's **Run command** or
     `vendor/bin/arco tinker`) or as plain SQL, for whoever can sign in as the owner. Then click **The owner ran it**.
   - **Let admin do it (temporary)**, offered only when `admin` created the owner role (it then holds `ADMIN OPTION`
     on it). DSQL has no `SET ROLE`, but a member of a role acts with its privileges, so the app runs
     `GRANT "<owner>" TO admin`, the grants, and `REVOKE "<owner>" FROM admin` right after. It needs your explicit
     agreement, and if a grant fails the app still runs the final `REVOKE`.
6. **Test**: the app signs in as the new role and says how many tables it can read.
7. **Save as a connection** adds it to **Your connections** (read-only when the role is read-only).

Tables created later by the app's migrations need the owner's grant again.

## Check access

**Access… → Check my access** lists every table of the current schema with ✓ or ✗ for `SELECT`, `INSERT`, `UPDATE`
and `DELETE` for the role you are signed in as, the table's owner, and the exact `GRANT` the owner would run for
what is missing. **Copy all grants** copies them.

## Remove a role

Connected as **admin**, **Access… → Remove a role**, pick the role (from the roles mapped to an IAM role), then
**Review**. DSQL refuses `DROP ROLE` while the role still holds privileges or a mapping, so the order is fixed:

1. The tables' owner revokes, one per table (with the same three ways as above):

   ```sql
   REVOKE ALL ON "<schema>"."<table>" FROM "readonly";
   ```

2. Then, as `admin` (the button is enabled once step 1 is done):

   ```sql
   AWS IAM REVOKE "readonly" FROM 'arn:aws:iam::<account>:role/<path>/<name>';
   REVOKE USAGE ON SCHEMA "<schema>" FROM "readonly";
   DROP ROLE "readonly";
   ```

## Permissions needed

| For | AWS permission |
| --- | --- |
| Connecting as `admin` | `dsql:DbConnectAdmin` on the cluster |
| Signing in as the new role (test, and teammates) | `dsql:DbConnect` on the cluster, for the mapped IAM role |
| Defaulting the IAM role ARN | `iam:GetRole` (optional; without it the ARN is built from the SSO pattern) |
| **Run it for me** | `lambda:InvokeFunction` on the console function (alias `live`) |

The app never creates or changes IAM users, roles or policies: `AWS IAM GRANT` is a mapping inside the database.
