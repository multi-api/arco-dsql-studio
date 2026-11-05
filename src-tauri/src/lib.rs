pub mod commands;
pub mod state;

use tauri::{Builder, Manager, Runtime};

use commands::{aws, db, roles, store};
use state::AppState;

/// The local store's file in the app's data directory. It holds saved connections, history and settings; never secrets.
const STORE_FILE: &str = "arco-dsql-studio.sqlite";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    with_commands(tauri::Builder::default())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let store = dsql_core::store::Store::open(&dir.join(STORE_FILE))?;
            app.manage(AppState::new(store));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Registers every command; shared by the app and the IPC tests.
pub fn with_commands<R: Runtime>(builder: Builder<R>) -> Builder<R> {
    builder.invoke_handler(tauri::generate_handler![
        aws::list_profiles,
        aws::sso_start_login,
        aws::sso_finish_login,
        aws::sso_list_accounts,
        aws::preview_sso_profile,
        aws::append_sso_profile,
        aws::sso_store_login,
        aws::parse_endpoint,
        aws::caller_identity,
        aws::list_clusters,
        aws::iam_role_for_profile,
        db::open_connection,
        db::close_connection,
        db::list_schemas,
        db::set_schema,
        db::list_tables,
        db::table_columns,
        db::table_indexes,
        db::table_page,
        db::looks_like_production,
        db::classify_sql,
        db::run_sql,
        db::cancel,
        db::preview_changes,
        db::apply_changes,
        db::explain_error,
        db::explain_error_offline,
        store::list_connections,
        store::save_connection,
        store::delete_connection,
        store::search_history,
        store::clear_history,
        store::get_setting,
        store::set_setting,
        roles::role_create_plan,
        roles::role_remove_plan,
        roles::role_takeover_plan,
        roles::role_admin_can_take_over,
        roles::role_run_admin,
        roles::role_run_owner_via_console,
        roles::role_console_function_name,
        roles::role_mappings,
    ])
}
