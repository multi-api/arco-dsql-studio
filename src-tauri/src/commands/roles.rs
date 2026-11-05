//! The role wizard: plans, running admin steps, the admin takeover and the Arco console.

use tauri::State;

use dsql_core::CoreError;
use dsql_core::console;
use dsql_core::exec::{MAX_ROWS, ScriptResult, StatementOutcome};
use dsql_core::identity;
use dsql_core::roles::{self, Access, RolePlan};

use crate::commands::db::run_tracked;
use crate::state::AppState;

#[tauri::command]
pub fn role_create_plan(
    role: String,
    iam_role_arn: String,
    schema: String,
    tables: Vec<String>,
    access: Access,
) -> Result<RolePlan, CoreError> {
    roles::create_plan(&role, &iam_role_arn, &schema, &tables, access)
}

#[tauri::command]
pub fn role_remove_plan(
    role: String,
    iam_role_arn: String,
    schema: String,
    tables: Vec<String>,
) -> Result<RolePlan, CoreError> {
    roles::remove_plan(&role, &iam_role_arn, &schema, &tables)
}

/// `sql` wrapped in `admin`'s temporary membership in `app_role`; run it with `role_run_admin` after the user agrees.
#[tauri::command]
pub fn role_takeover_plan(app_role: String, sql: Vec<String>) -> Vec<String> {
    roles::grant_owner_privileges_via_admin_plan(&app_role, &sql)
}

#[tauri::command]
pub async fn role_admin_can_take_over(
    state: State<'_, AppState>,
    id: String,
    app_role: String,
) -> Result<bool, CoreError> {
    let s = state.connection(&id)?.pool().get().await?;
    roles::admin_can_take_over(&s, &app_role).await
}

/// Runs the statements one at a time (DSQL allows one DDL per transaction) and stops at the first error.
#[tauri::command]
pub async fn role_run_admin(
    state: State<'_, AppState>,
    id: String,
    sql: Vec<String>,
) -> Result<ScriptResult, CoreError> {
    let conn = state.connection(&id)?;
    let mut all = ScriptResult {
        outcomes: Vec::new(),
        duration_ms: 0,
    };
    for stmt in &sql {
        let result = run_tracked(&conn, stmt, MAX_ROWS).await;
        all.duration_ms += result.duration_ms;
        let failed = result
            .outcomes
            .iter()
            .any(|o| matches!(o, StatementOutcome::Error(_)));
        all.outcomes.extend(result.outcomes);
        if failed {
            break;
        }
    }
    Ok(all)
}

/// Invokes an Arco Cloud console Lambda with the user's own profile; returns the command's output.
#[tauri::command]
pub async fn role_run_owner_via_console(
    profile: String,
    region: String,
    function: String,
    php: String,
) -> Result<String, CoreError> {
    let cfg = identity::sdk_config(&profile, &region).await?;
    console::run_tinker(&cfg, &function, &php).await
}

#[tauri::command]
pub fn role_console_function_name(schema: String) -> String {
    console::console_function_name(&schema)
}

#[tauri::command]
pub async fn role_mappings(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<(String, String)>, CoreError> {
    let s = state.connection(&id)?.pool().get().await?;
    roles::existing_mappings(&s).await
}
