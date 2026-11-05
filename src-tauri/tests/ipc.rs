//! The commands called over IPC with Tauri's mock runtime, the way the frontend calls them: argument names, JSON
//! shapes and errors. The live part runs only with the integration variables of `docs/development.md`.

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tauri::test::{INVOKE_KEY, MockRuntime, mock_builder, mock_context, noop_assets};
use tauri::webview::InvokeRequest;
use tauri::{WebviewWindow, WebviewWindowBuilder};

use arco_dsql_studio_lib::state::AppState;
use arco_dsql_studio_lib::with_commands;
use dsql_core::store::Store;

/// The origin of the app's own pages. Commands are allowed only from it, and it differs by platform.
const APP_ORIGIN: &str = if cfg!(windows) {
    "http://tauri.localhost"
} else {
    "tauri://localhost"
};

fn app(dir: &tempfile::TempDir) -> (tauri::App<MockRuntime>, WebviewWindow<MockRuntime>) {
    let store = Store::open(&dir.path().join("store.sqlite")).unwrap();
    let app = with_commands(mock_builder())
        .manage(AppState::new(store))
        .build(mock_context(noop_assets()))
        .unwrap();
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    (app, webview)
}

fn call<T: DeserializeOwned>(
    webview: &WebviewWindow<MockRuntime>,
    cmd: &str,
    args: Value,
) -> Result<T, Value> {
    tauri::test::get_ipc_response(
        webview,
        InvokeRequest {
            cmd: cmd.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: APP_ORIGIN.parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|body| body.deserialize::<T>().unwrap())
}

#[test]
fn ipc_offline_commands() {
    let dir = tempfile::tempdir().unwrap();
    let (_app, w) = app(&dir);

    let classified: Value =
        call(&w, "classify_sql", json!({ "sql": "select 1; truncate t" })).unwrap();
    assert_eq!(classified["statements"][1]["kind"], "Schema");
    assert_eq!(
        classified["statements"][1]["findings"][0]["rule_id"],
        "truncate"
    );

    let at: Option<String> = call(
        &w,
        "statement_at",
        json!({ "sql": "select 1; select 2", "cursor": 12 }),
    )
    .unwrap();
    assert_eq!(at.as_deref(), Some("select 2"));

    let batches: Vec<Vec<String>> = call(
        &w,
        "preview_changes",
        json!({ "changeset": {
            "schema": "s", "table": "t", "pk": ["id"], "column_types": { "id": "bigint" },
            "updates": [{ "pk_values": { "id": "1" }, "set": { "name": null } }],
            "inserts": [], "deletes": []
        } }),
    )
    .unwrap();
    assert_eq!(
        batches,
        [[r#"UPDATE "s"."t" SET "name" = NULL WHERE "id" = '1'::bigint"#]]
    );

    let plan: Value = call(
        &w,
        "role_create_plan",
        json!({ "role": "readonly", "iamRoleArn": "arn:aws:iam::111122223333:role/r",
                "schema": "app", "tables": ["users"], "access": "ReadOnly" }),
    )
    .unwrap();
    assert_eq!(
        plan["owner_sql"][0],
        r#"GRANT SELECT ON "app"."users" TO "readonly""#
    );

    let saved = json!({ "connection": {
        "id": null, "name": "staging", "profile": "p", "region": "us-east-1", "cluster_id": "c",
        "host": "c.dsql.us-east-1.on.aws", "role": "admin", "schema": "app", "production": false, "read_only": true
    } });
    let id: i64 = call(&w, "save_connection", saved).unwrap();
    let list: Vec<Value> = call(&w, "list_connections", json!({})).unwrap();
    assert_eq!((list.len(), list[0]["id"].as_i64()), (1, Some(id)));
    call::<()>(
        &w,
        "set_setting",
        json!({ "name": "theme", "value": "dark" }),
    )
    .unwrap();
    let theme: Option<String> = call(&w, "get_setting", json!({ "name": "theme" })).unwrap();
    assert_eq!(theme.as_deref(), Some("dark"));

    // Errors arrive as the serialised CoreError.
    let err = call::<Value>(
        &w,
        "run_sql",
        json!({ "id": "conn-404", "sql": "select 1" }),
    )
    .unwrap_err();
    assert_eq!(err, json!({ "Config": "connection conn-404 is not open" }));
    let err =
        call::<Value>(&w, "sso_list_accounts", json!({ "loginId": "login-404" })).unwrap_err();
    assert!(
        err["Config"]
            .as_str()
            .unwrap()
            .contains("no longer pending"),
        "{err}"
    );
}

/// Opens an admin connection on the test schema and exercises the catalog, scripts, history, a table page denied by
/// DSQL, its explanation, and the read-only guard.
#[test]
fn ipc_live_connection() {
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let (Some(profile), Some(region), Some(host), Some(schema)) = (
        var("AWS_PROFILE"),
        var("DSQL_REGION"),
        var("DSQL_HOST"),
        var("DSQL_TEST_SCHEMA"),
    ) else {
        eprintln!("skipped: AWS_PROFILE, DSQL_REGION, DSQL_HOST and DSQL_TEST_SCHEMA not all set");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (_app, w) = app(&dir);
    let saved = |read_only: bool| {
        json!({ "saved": {
            "id": null, "name": "test", "profile": profile, "region": region, "cluster_id": "",
            "host": host, "role": "admin", "schema": null, "production": false, "read_only": read_only
        } })
    };

    let handle: Value = call(&w, "open_connection", saved(false)).unwrap();
    let id = handle["id"].as_str().unwrap().to_string();
    assert_eq!(handle["current_user"], "admin");
    println!("opened {handle}");

    let schemas: Vec<Value> = call(&w, "list_schemas", json!({ "id": id })).unwrap();
    assert!(schemas.iter().any(|s| s["name"] == schema.as_str()));
    let tables: Vec<Value> =
        call(&w, "list_tables", json!({ "id": id, "schema": schema })).unwrap();
    let table = tables[0]["name"].as_str().unwrap().to_string();

    let check: Value = call(&w, "check_sql", json!({ "id": id, "sql": "truncate t" })).unwrap();
    assert_eq!(
        (
            check["blocked"].as_bool(),
            check["needs_confirmation"].as_bool()
        ),
        (Some(false), Some(false))
    );
    assert_eq!(
        check["classified"]["statements"][0]["findings"][0]["rule_id"],
        "truncate"
    );

    let script: Value = call(
        &w,
        "run_sql",
        json!({ "id": id, "sql": "select 1 as a; select 'x' as b" }),
    )
    .unwrap();
    assert_eq!(script["outcomes"][1]["Rows"]["rows"], json!([["x"]]));
    let history: Vec<Value> = call(&w, "search_history", json!({ "query": "'x'" })).unwrap();
    assert_eq!(history[0]["rows"], 2);

    // admin cannot read the owner's tables: DSQL's error comes back, and explain_error suggests the grant.
    let page =
        json!({ "offset": 0, "limit": 10, "order_by": [], "filters": [], "where_sql": null });
    let err = call::<Value>(
        &w,
        "table_page",
        json!({ "id": id, "schema": schema, "table": table, "page": page }),
    )
    .unwrap_err();
    assert_eq!(err["Db"]["sqlstate"], "42501", "{err}");
    let explanation: Value = call(&w, "explain_error", json!({ "id": id, "err": err })).unwrap();
    println!("explained: {explanation}");
    assert!(
        explanation["fix"]["ShowGrant"]["sql"]
            .as_str()
            .unwrap()
            .starts_with("GRANT SELECT")
    );

    call::<()>(&w, "close_connection", json!({ "id": id })).unwrap();

    let ro: Value = call(&w, "open_connection", saved(true)).unwrap();
    let ro_id = ro["id"].as_str().unwrap();
    let check: Value = call(
        &w,
        "check_sql",
        json!({ "id": ro_id, "sql": "delete from t" }),
    )
    .unwrap();
    assert_eq!(check["blocked"], true);
    let err = call::<Value>(
        &w,
        "run_sql",
        json!({ "id": ro_id, "sql": "delete from t" }),
    )
    .unwrap_err();
    assert!(
        err["Config"].as_str().unwrap().contains("read-only"),
        "{err}"
    );
    let ok: Value = call(&w, "run_sql", json!({ "id": ro_id, "sql": "select 1" })).unwrap();
    assert!(ok["outcomes"][0]["Rows"].is_object());
}
