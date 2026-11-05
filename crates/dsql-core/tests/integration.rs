//! Integration tests against a real AWS account and Aurora DSQL cluster.
//!
//! Each test is skipped (passes without doing anything) unless the environment variables it needs are set; see
//! `docs/development.md`. Never commit real values.

use std::time::{Duration, Instant};

use dsql_core::conn::{self, Pool, Target};
use dsql_core::token::DbRole;
use dsql_core::{catalog, clusters, identity};

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

/// An admin target for `DSQL_HOST` in `DSQL_TEST_SCHEMA`, or `None` (test skipped).
fn admin_target() -> Option<Target> {
    let (profile, region) = aws_env()?;
    let host = std::env::var("DSQL_HOST").ok().filter(|v| !v.is_empty())?;
    let schema = std::env::var("DSQL_TEST_SCHEMA")
        .ok()
        .filter(|v| !v.is_empty())?;
    Some(Target {
        profile,
        region,
        host,
        role: DbRole::Admin,
        schema: Some(schema),
    })
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

#[tokio::test]
async fn it_connect_admin_sets_search_path() {
    let Some(target) = admin_target() else {
        eprintln!("skipped: AWS_PROFILE, DSQL_REGION, DSQL_HOST and DSQL_TEST_SCHEMA not all set");
        return;
    };
    let session = conn::connect(&target).await.expect("connect as admin");
    let row = session
        .client
        .query_one("select current_user::text, current_schema()::text", &[])
        .await
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "admin");
    assert_eq!(Some(row.get::<_, String>(1)), target.schema);
}

#[tokio::test]
async fn it_cancel_closes_the_connection() {
    let Some(target) = admin_target() else {
        eprintln!("skipped: AWS_PROFILE, DSQL_REGION, DSQL_HOST and DSQL_TEST_SCHEMA not all set");
        return;
    };
    let pool = Pool::new(target, 2);
    let session = pool.get().await.expect("connect");
    let started = Instant::now();
    let (result, _) = tokio::join!(session.client.simple_query("select pg_sleep(30)"), async {
        tokio::time::sleep(Duration::from_secs(1)).await;
        session.cancel().await;
    });
    let elapsed = started.elapsed();
    println!(
        "cancelled after {} ms: {:?}",
        elapsed.as_millis(),
        result.as_ref().map(|_| ()).map_err(|e| e.to_string())
    );
    assert!(result.is_err());
    assert!(elapsed < Duration::from_secs(10), "cancel took {elapsed:?}");
    assert!(session.is_closed());
    drop(session);

    // The pool does not hand the closed session out again.
    let next = pool.get().await.expect("reconnect");
    assert!(!next.is_closed());
    next.client
        .simple_query("select 1")
        .await
        .expect("new session works");
}

#[tokio::test]
async fn it_pool_reuses_idle_sessions() {
    let Some(target) = admin_target() else {
        eprintln!("skipped: AWS_PROFILE, DSQL_REGION, DSQL_HOST and DSQL_TEST_SCHEMA not all set");
        return;
    };
    let pool = Pool::new(target, 2);
    let first = pool.get().await.expect("connect").opened_at;
    let second = pool.get().await.expect("reuse");
    assert_eq!(second.opened_at, first, "the idle session should be reused");
    let third = pool.get().await.expect("second slot");
    assert_ne!(third.opened_at, first);
}

#[tokio::test]
async fn it_lists_test_schema_tables() {
    let Some(target) = admin_target() else {
        eprintln!("skipped: AWS_PROFILE, DSQL_REGION, DSQL_HOST and DSQL_TEST_SCHEMA not all set");
        return;
    };
    let schema = target.schema.clone().unwrap();
    let session = conn::connect(&target).await.expect("connect as admin");
    let started = Instant::now();

    let schemas = catalog::schemas(&session).await.expect("schemas");
    let names: Vec<&str> = schemas.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&schema.as_str()), "{names:?}");
    for system in ["pg_catalog", "information_schema", "sys", "pg_toast"] {
        assert!(!names.contains(&system), "{names:?}");
    }

    let tables = catalog::tables(&session, &schema).await.expect("tables");
    assert!(!tables.is_empty());
    let owner = &tables[0].owner;
    assert_ne!(owner, "admin");
    for t in &tables {
        assert_eq!(&t.owner, owner, "all tables belong to the app role");
        assert!(!t.can_select, "admin cannot read {} until granted", t.name);
    }

    // Catalog queries work on tables admin cannot read, including hostile names.
    for t in &tables {
        let cols = catalog::columns(&session, &schema, &t.name)
            .await
            .expect("columns");
        let pk = catalog::primary_key(&session, &schema, &t.name)
            .await
            .expect("primary key");
        let idx = catalog::indexes(&session, &schema, &t.name)
            .await
            .expect("indexes");
        assert_eq!(cols.iter().filter(|c| c.is_pk).count(), pk.len());
        println!(
            "{}: {} columns, pk {:?}, indexes {:?}",
            t.name,
            cols.len(),
            pk,
            idx
        );
    }
    println!("catalog read in {} ms", started.elapsed().as_millis());

    let role_default = catalog::role_search_path(&session)
        .await
        .expect("role config");
    println!("admin's own search_path setting: {role_default:?}");
}

#[tokio::test]
async fn it_run_script_multi_statement() {
    use dsql_core::exec::{StatementOutcome, run_script};
    let Some(target) = admin_target() else {
        eprintln!("skipped: AWS_PROFILE, DSQL_REGION, DSQL_HOST and DSQL_TEST_SCHEMA not all set");
        return;
    };
    let session = conn::connect(&target).await.expect("connect as admin");

    let script = "select 1 as a; \
                  select null::text as n, ''::text as e, 'NULL'::text as s; \
                  select generate_series(1, 25) as g; \
                  select * from users; \
                  select 2";
    let result = run_script(&session, script, 10).await;
    println!(
        "{} outcomes in {} ms",
        result.outcomes.len(),
        result.duration_ms
    );
    assert_eq!(
        result.outcomes.len(),
        4,
        "stops at the first error: {:?}",
        result.outcomes
    );
    let StatementOutcome::Rows(one) = &result.outcomes[0] else {
        panic!("{:?}", result.outcomes[0])
    };
    assert_eq!(
        (one.columns.clone(), one.rows.clone()),
        (vec!["a".to_string()], vec![vec![Some("1".to_string())]])
    );
    let StatementOutcome::Rows(nulls) = &result.outcomes[1] else {
        panic!("{:?}", result.outcomes[1])
    };
    assert_eq!(
        nulls.rows,
        vec![vec![None, Some(String::new()), Some("NULL".to_string())]]
    );
    let StatementOutcome::Rows(series) = &result.outcomes[2] else {
        panic!("{:?}", result.outcomes[2])
    };
    assert_eq!(series.rows.len(), 10);
    assert!(series.truncated);
    let StatementOutcome::Error(dsql_core::CoreError::Db { sqlstate, .. }) = &result.outcomes[3]
    else {
        panic!("{:?}", result.outcomes[3])
    };
    assert_eq!(
        sqlstate.as_deref(),
        Some("42501"),
        "admin cannot read the app's tables"
    );

    // The session is still usable after a truncated result and an error.
    let after = run_script(&session, "select 'ok'", 10).await;
    assert!(
        matches!(&after.outcomes[0], StatementOutcome::Rows(r) if r.rows == vec![vec![Some("ok".to_string())]])
    );

    // Two DDL statements in one script work because each statement runs on its own (DSQL allows one DDL per
    // transaction). Scratch tables in public, removed at the end.
    let _ = run_script(
        &session,
        "drop table if exists public.it_exec_a; drop table if exists public.it_exec_b",
        10,
    )
    .await;
    let ddl = run_script(
        &session,
        "create table public.it_exec_a (id int primary key); create table public.it_exec_b (id int primary key); \
         insert into public.it_exec_a values (1), (2); drop table public.it_exec_a; drop table public.it_exec_b",
        10,
    )
    .await;
    let tags: Vec<String> = ddl
        .outcomes
        .iter()
        .map(|o| match o {
            StatementOutcome::Command { tag, rows_affected } => format!("{tag} {rows_affected}"),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(
        tags,
        [
            "CREATE TABLE 0",
            "CREATE TABLE 0",
            "INSERT 2",
            "DROP TABLE 0",
            "DROP TABLE 0"
        ]
    );
}

#[tokio::test]
async fn it_explains_real_errors() {
    use dsql_core::exec::{StatementOutcome, run_script};
    use dsql_core::explain::{ExplainContext, FixAction, explain};
    let Some(target) = admin_target() else {
        eprintln!("skipped: AWS_PROFILE, DSQL_REGION, DSQL_HOST and DSQL_TEST_SCHEMA not all set");
        return;
    };
    let schema = target.schema.clone().unwrap();
    let in_public = Target {
        schema: None,
        ..target.clone()
    };
    let session = conn::connect(&in_public).await.expect("connect as admin");
    let ctx = ExplainContext {
        role: "admin".into(),
        search_path: Some("public".into()),
        caller_arn: None,
        is_admin: true,
        known_tables: catalog::tables(&session, &schema).await.expect("tables"),
    };
    let error_of = |r: dsql_core::exec::ScriptResult| match r.outcomes.into_iter().last() {
        Some(StatementOutcome::Error(e)) => e,
        other => panic!("expected an error, got {other:?}"),
    };

    let missing = error_of(run_script(&session, "select * from users", 10).await);
    let e = explain(&missing, &ctx);
    println!("{}: {}", e.title, e.body);
    assert_eq!(e.fix, FixAction::SwitchSchema(schema.clone()));

    let denied = error_of(run_script(&session, &format!("select * from {schema}.users"), 10).await);
    let e = explain(&denied, &ctx);
    println!("{}: {}", e.title, e.body);
    let FixAction::ShowGrant { owner, sql, .. } = e.fix else {
        panic!("{:?}", e.fix)
    };
    assert_ne!(owner, "admin");
    assert_eq!(
        sql,
        format!("GRANT SELECT ON \"{schema}\".\"users\" TO \"admin\";")
    );

    let nobody = Target {
        role: DbRole::Custom("it_no_such_role".into()),
        ..in_public
    };
    let refused = conn::connect(&nobody)
        .await
        .err()
        .expect("an unmapped role is refused");
    let e = explain(
        &refused,
        &ExplainContext {
            role: "it_no_such_role".into(),
            is_admin: false,
            ..ctx
        },
    );
    println!("{}: {}", e.title, e.body);
    assert!(e.body.contains("AWS IAM GRANT"));
}
