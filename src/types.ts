// Mirrors of the Rust types the Tauri commands take and return (serde's default representation: snake_case fields,
// externally tagged enums). Keep in sync with `crates/dsql-core` and `src-tauri/src/commands`.

export type AwsErrorKind = "SsoExpired" | "AccessDenied" | "NotAvailableInRegion" | "Other";

export type CoreError =
  | { Aws: { kind: AwsErrorKind; message: string } }
  | { Db: { sqlstate: string | null; message: string; detail: string | null; hint: string | null } }
  | { Io: string }
  | { Config: string }
  | "Cancelled";

// AWS

export type ProfileKind = "Sso" | "AccessKeys" | "AssumeRole" | "Process" | "Other";

export interface ProfileInfo {
  name: string;
  kind: ProfileKind;
  sso_session: string | null;
  sso_start_url: string | null;
  sso_region: string | null;
  sso_account_id: string | null;
  sso_role_name: string | null;
  region: string | null;
}

export interface NewSsoProfile {
  profile: string;
  session: string;
  start_url: string;
  sso_region: string;
  account_id: string;
  role_name: string;
  region: string;
}

export interface SsoLoginStarted {
  login_id: string;
  verification_uri_complete: string;
  user_code: string;
  expires_in: number;
}

export interface SsoAccountRole {
  account_id: string;
  account_name: string;
  role_name: string;
}

export interface CallerIdentity {
  account: string;
  arn: string;
  user_id: string;
}

export interface ClusterInfo {
  identifier: string;
  region: string;
  status: string;
  endpoint: string;
  arn: string;
  tags: Record<string, string>;
}

export interface ClusterList {
  clusters: ClusterInfo[];
  failed: { region: string; error: CoreError }[];
}

export interface IamRole {
  arn: string;
  guessed: boolean;
}

// Database

export interface SavedConnection {
  id: number | null;
  name: string;
  profile: string;
  region: string;
  cluster_id: string;
  host: string;
  /** "admin" or a custom role name. */
  role: string;
  schema: string | null;
  production: boolean;
  read_only: boolean;
}

export interface ConnectionHandle {
  id: string;
  current_user: string;
  schema: string | null;
}

export interface SchemaInfo {
  name: string;
  owner: string;
}

export type TableKind = "Table" | "View" | "Sequence";

export interface TableInfo {
  schema: string;
  name: string;
  kind: TableKind;
  owner: string;
  can_select: boolean;
  can_insert: boolean;
  can_update: boolean;
  can_delete: boolean;
}

export interface ColumnInfo {
  name: string;
  data_type: string;
  nullable: boolean;
  default: string | null;
  is_pk: boolean;
  ordinal: number;
}

export interface IndexInfo {
  name: string;
  columns: string[];
  unique: boolean;
}

export interface ResultSet {
  columns: string[];
  /** null is SQL NULL; "" an empty string; "NULL" the text NULL. */
  rows: (string | null)[][];
  truncated: boolean;
}

export type StatementOutcome =
  | { Rows: ResultSet }
  | { Command: { tag: string; rows_affected: number } }
  | { Error: CoreError };

export interface ScriptResult {
  outcomes: StatementOutcome[];
  duration_ms: number;
}

export type FilterOp = "Eq" | "Ne" | "Lt" | "Gt" | "Contains" | "IsNull" | "IsNotNull";

export interface Filter {
  column: string;
  op: FilterOp;
  value: string | null;
}

export interface PageRequest {
  offset: number;
  limit: number;
  /** Column and ascending. */
  order_by: [string, boolean][];
  filters: Filter[];
  where_sql: string | null;
}

export interface TablePage {
  result: ResultSet;
  sql: string;
}

export type StatementKind = "Read" | "Write" | "Schema" | "Transaction" | "Other";

export interface Finding {
  rule_id: string;
  message: string;
  alternative: string;
  docs_url: string | null;
}

export interface ClassifiedStatement {
  sql: string;
  kind: StatementKind;
  findings: Finding[];
}

export interface Classified {
  statements: ClassifiedStatement[];
}

export interface SqlCheck {
  classified: Classified;
  /** The connection is read-only and the SQL writes or changes the schema. */
  blocked: boolean;
  /** The connection is production and the SQL writes or changes the schema. */
  needs_confirmation: boolean;
}

/** Column name to value; null is SQL NULL. */
export type Values = Record<string, string | null>;

export interface RowUpdate {
  pk_values: Values;
  set: Values;
}

export interface ChangeSet {
  schema: string;
  table: string;
  pk: string[];
  column_types: Record<string, string>;
  updates: RowUpdate[];
  inserts: Values[];
  deletes: Values[];
}

export interface ApplyReport {
  batches_applied: number;
  batches_total: number;
  rows_affected: number;
  rows_missing: number;
  retries: number;
  error: CoreError | null;
}

export type FixAction =
  | { SwitchSchema: string }
  | { ShowGrant: { table: string; owner: string; sql: string } }
  | "SignIn"
  | "Retry"
  | "Rollback"
  | "None";

export interface Explanation {
  title: string;
  body: string;
  fix: FixAction;
}

// Store

export interface HistoryEntry {
  id: number | null;
  connection_id: number | null;
  sql: string;
  /** RFC 3339, UTC. */
  started_at: string;
  duration_ms: number;
  rows: number | null;
  error: string | null;
}

// Roles

export type Access = "ReadOnly" | "ReadWrite";

export interface RolePlan {
  admin_sql: string[];
  owner_sql: string[];
  owner_tinker: string;
}
