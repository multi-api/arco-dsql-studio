//! Saved connections, query history and settings in the local SQLite store.

use tauri::State;

use dsql_core::CoreError;
use dsql_core::store::{HistoryEntry, SavedConnection};

use crate::state::AppState;

/// History entries returned per search.
const HISTORY_LIMIT: usize = 200;

#[tauri::command]
pub fn list_connections(state: State<'_, AppState>) -> Result<Vec<SavedConnection>, CoreError> {
    state.with_store(|s| s.list_connections())
}

/// Saves (inserts or updates) a connection and returns its id.
#[tauri::command]
pub fn save_connection(
    state: State<'_, AppState>,
    connection: SavedConnection,
) -> Result<i64, CoreError> {
    state.with_store(|s| s.save_connection(&connection))
}

#[tauri::command]
pub fn delete_connection(state: State<'_, AppState>, id: i64) -> Result<(), CoreError> {
    state.with_store(|s| s.delete_connection(id))
}

#[tauri::command]
pub fn search_history(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<HistoryEntry>, CoreError> {
    state.with_store(|s| s.search_history(&query, HISTORY_LIMIT))
}

#[tauri::command]
pub fn clear_history(state: State<'_, AppState>) -> Result<(), CoreError> {
    state.with_store(|s| s.clear_history())
}

#[tauri::command]
pub fn get_setting(state: State<'_, AppState>, name: String) -> Result<Option<String>, CoreError> {
    state.with_store(|s| s.get_setting(&name))
}

#[tauri::command]
pub fn set_setting(
    state: State<'_, AppState>,
    name: String,
    value: String,
) -> Result<(), CoreError> {
    state.with_store(|s| s.set_setting(&name, &value))
}
