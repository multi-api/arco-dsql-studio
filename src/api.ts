// Typed wrappers for the Tauri commands in `src-tauri/src/commands`. Tauri passes arguments by camelCase name; a
// failed command rejects with a `CoreError`.

import { invoke } from "@tauri-apps/api/core";

import type {
  Access,
  ApplyReport,
  CallerIdentity,
  ChangeSet,
  Classified,
  ClusterList,
  ColumnInfo,
  ConnectionHandle,
  CoreError,
  Explanation,
  HistoryEntry,
  IamRole,
  IndexInfo,
  NewSsoProfile,
  PageRequest,
  ProfileInfo,
  RolePlan,
  SavedConnection,
  SchemaInfo,
  ScriptResult,
  SsoAccountRole,
  SsoLoginStarted,
  TableInfo,
  TablePage,
} from "./types";

// AWS

export const listProfiles = () => invoke<ProfileInfo[]>("list_profiles");

export const ssoStartLogin = (startUrl: string, ssoRegion: string) =>
  invoke<SsoLoginStarted>("sso_start_login", { startUrl, ssoRegion });

/** Resolves once the user approves the sign-in in the browser. */
export const ssoFinishLogin = (loginId: string) => invoke<void>("sso_finish_login", { loginId });

export const ssoListAccounts = (loginId: string) =>
  invoke<SsoAccountRole[]>("sso_list_accounts", { loginId });

export const previewSsoProfile = (newProfile: NewSsoProfile) =>
  invoke<string>("preview_sso_profile", { newProfile });

export const appendSsoProfile = (newProfile: NewSsoProfile, loginId: string | null) =>
  invoke<void>("append_sso_profile", { newProfile, loginId });

/** Saves an approved sign-in for an existing SSO profile, like `aws sso login --profile`. */
export const ssoStoreLogin = (loginId: string, ssoSession: string | null) =>
  invoke<void>("sso_store_login", { loginId, ssoSession });

/** `[clusterId, region]`, or null when `host` is not a DSQL endpoint. */
export const parseEndpoint = (host: string) =>
  invoke<[string, string] | null>("parse_endpoint", { host });

export const callerIdentity = (profile: string, region: string | null = null) =>
  invoke<CallerIdentity>("caller_identity", { profile, region });

export const listClusters = (profile: string, regions: string[] | null = null) =>
  invoke<ClusterList>("list_clusters", { profile, regions });

export const iamRoleForProfile = (profile: string, region: string | null = null) =>
  invoke<IamRole>("iam_role_for_profile", { profile, region });

// Database

export const openConnection = (saved: SavedConnection) =>
  invoke<ConnectionHandle>("open_connection", { saved });

export const closeConnection = (id: string) => invoke<void>("close_connection", { id });

export const listSchemas = (id: string) => invoke<SchemaInfo[]>("list_schemas", { id });

export const setSchema = (id: string, schema: string) => invoke<void>("set_schema", { id, schema });

export const listTables = (id: string, schema: string) =>
  invoke<TableInfo[]>("list_tables", { id, schema });

export const tableColumns = (id: string, schema: string, table: string) =>
  invoke<ColumnInfo[]>("table_columns", { id, schema, table });

export const tableIndexes = (id: string, schema: string, table: string) =>
  invoke<IndexInfo[]>("table_indexes", { id, schema, table });

export const tablePage = (id: string, schema: string, table: string, page: PageRequest) =>
  invoke<TablePage>("table_page", { id, schema, table, page });

export const looksLikeProduction = (schema: string, tags: Record<string, string>) =>
  invoke<boolean>("looks_like_production", { schema, tags });

export const classifySql = (sql: string) => invoke<Classified>("classify_sql", { sql });

export const runSql = (id: string, sql: string) => invoke<ScriptResult>("run_sql", { id, sql });

export const cancel = (id: string) => invoke<void>("cancel", { id });

export const previewChanges = (changeset: ChangeSet) =>
  invoke<string[][]>("preview_changes", { changeset });

export const applyChanges = (id: string, changeset: ChangeSet) =>
  invoke<ApplyReport>("apply_changes", { id, changeset });

export const explainError = (id: string, err: CoreError) =>
  invoke<Explanation>("explain_error", { id, err });

/** For errors with no open connection: a failed connect (pass the connection) or an AWS error (pass null). */
export const explainErrorOffline = (saved: SavedConnection | null, err: CoreError) =>
  invoke<Explanation>("explain_error_offline", { saved, err });

// Store

export const listConnections = () => invoke<SavedConnection[]>("list_connections");

/** Inserts or updates; returns the connection's id. */
export const saveConnection = (connection: SavedConnection) =>
  invoke<number>("save_connection", { connection });

export const deleteConnection = (id: number) => invoke<void>("delete_connection", { id });

export const searchHistory = (query: string) => invoke<HistoryEntry[]>("search_history", { query });

export const clearHistory = () => invoke<void>("clear_history");

export const getSetting = (name: string) => invoke<string | null>("get_setting", { name });

export const setSetting = (name: string, value: string) =>
  invoke<void>("set_setting", { name, value });

// Roles

export const roleCreatePlan = (
  role: string,
  iamRoleArn: string,
  schema: string,
  tables: string[],
  access: Access,
) => invoke<RolePlan>("role_create_plan", { role, iamRoleArn, schema, tables, access });

export const roleRemovePlan = (role: string, iamRoleArn: string, schema: string, tables: string[]) =>
  invoke<RolePlan>("role_remove_plan", { role, iamRoleArn, schema, tables });

export const roleTakeoverPlan = (appRole: string, sql: string[]) =>
  invoke<string[]>("role_takeover_plan", { appRole, sql });

export const roleAdminCanTakeOver = (id: string, appRole: string) =>
  invoke<boolean>("role_admin_can_take_over", { id, appRole });

export const roleRunAdmin = (id: string, sql: string[]) =>
  invoke<ScriptResult>("role_run_admin", { id, sql });

export const roleRunOwnerViaConsole = (
  profile: string,
  region: string,
  fn: string,
  php: string,
) => invoke<string>("role_run_owner_via_console", { profile, region, function: fn, php });

export const roleConsoleFunctionName = (schema: string) =>
  invoke<string>("role_console_function_name", { schema });

export const roleMappings = (id: string) => invoke<[string, string][]>("role_mappings", { id });
