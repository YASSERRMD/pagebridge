//! Per-adapter audit-table schemas.
//!
//! Pagebridge adapters store their own data plus an optional
//! `pagebridge_audit` table that holds the local chain. The schema is
//! adapter-specific (Postgres uses BYTEA, SQLite uses BLOB, MongoDB uses
//! BinData, etc.); this module emits canonical DDL strings each adapter
//! can run during migrations.

/// Postgres DDL for the audit table.
pub const POSTGRES_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS pagebridge_audit (
    workspace_id   TEXT      NOT NULL,
    event_id       TEXT      NOT NULL,
    timestamp_ns   NUMERIC   NOT NULL,
    action         TEXT      NOT NULL,
    outcome        TEXT      NOT NULL,
    adapter        TEXT      NOT NULL,
    llm_provider   TEXT,
    llm_model      TEXT,
    input_tokens   INTEGER   NOT NULL DEFAULT 0,
    output_tokens  INTEGER   NOT NULL DEFAULT 0,
    latency_ms     INTEGER   NOT NULL DEFAULT 0,
    sensitivity    TEXT,
    parent_event   TEXT,
    prev_hash      BYTEA     NOT NULL,
    event_hash     BYTEA     NOT NULL,
    signature      BYTEA     NOT NULL,
    key_id         TEXT      NOT NULL,
    body           JSONB     NOT NULL,
    PRIMARY KEY (workspace_id, event_id)
);
CREATE INDEX IF NOT EXISTS pagebridge_audit_ts_idx
    ON pagebridge_audit (workspace_id, timestamp_ns);

CREATE TABLE IF NOT EXISTS pagebridge_audit_batches (
    workspace_id      TEXT      NOT NULL,
    batch_id          BIGINT    NOT NULL,
    first_event_id    TEXT      NOT NULL,
    last_event_id     TEXT      NOT NULL,
    leaf_count        INTEGER   NOT NULL,
    root              BYTEA     NOT NULL,
    anchored_log      TEXT,
    anchored_index    BIGINT,
    PRIMARY KEY (workspace_id, batch_id)
);
"#;

/// SQLite DDL for the audit table.

pub const SQLITE_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS pagebridge_audit (
    workspace_id   TEXT      NOT NULL,
    event_id       TEXT      NOT NULL,
    timestamp_ns   INTEGER   NOT NULL,
    action         TEXT      NOT NULL,
    outcome        TEXT      NOT NULL,
    adapter        TEXT      NOT NULL,
    llm_provider   TEXT,
    llm_model      TEXT,
    input_tokens   INTEGER   NOT NULL DEFAULT 0,
    output_tokens  INTEGER   NOT NULL DEFAULT 0,
    latency_ms     INTEGER   NOT NULL DEFAULT 0,
    sensitivity    TEXT,
    parent_event   TEXT,
    prev_hash      BLOB      NOT NULL,
    event_hash     BLOB      NOT NULL,
    signature      BLOB      NOT NULL,
    key_id         TEXT      NOT NULL,
    body           TEXT      NOT NULL,
    PRIMARY KEY (workspace_id, event_id)
);
CREATE INDEX IF NOT EXISTS pagebridge_audit_ts_idx
    ON pagebridge_audit (workspace_id, timestamp_ns);

CREATE TABLE IF NOT EXISTS pagebridge_audit_batches (
    workspace_id      TEXT      NOT NULL,
    batch_id          INTEGER   NOT NULL,
    first_event_id    TEXT      NOT NULL,
    last_event_id     TEXT      NOT NULL,
    leaf_count        INTEGER   NOT NULL,
    root              BLOB      NOT NULL,
    anchored_log      TEXT,
    anchored_index    INTEGER,
    PRIMARY KEY (workspace_id, batch_id)
);
"#;

/// MySQL/MariaDB DDL for the audit table.

pub const MYSQL_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS pagebridge_audit (
    workspace_id   VARCHAR(64)  NOT NULL,
    event_id       VARCHAR(32)  NOT NULL,
    timestamp_ns   BIGINT       NOT NULL,
    action         VARCHAR(32)  NOT NULL,
    outcome        VARCHAR(32)  NOT NULL,
    adapter        VARCHAR(64)  NOT NULL,
    llm_provider   VARCHAR(64),
    llm_model      VARCHAR(128),
    input_tokens   INT          NOT NULL DEFAULT 0,
    output_tokens  INT          NOT NULL DEFAULT 0,
    latency_ms     INT          NOT NULL DEFAULT 0,
    sensitivity    VARCHAR(64),
    parent_event   VARCHAR(32),
    prev_hash      VARBINARY(32) NOT NULL,
    event_hash     VARBINARY(32) NOT NULL,
    signature      VARBINARY(64) NOT NULL,
    key_id         VARCHAR(64)  NOT NULL,
    body           JSON         NOT NULL,
    PRIMARY KEY (workspace_id, event_id),
    INDEX pagebridge_audit_ts_idx (workspace_id, timestamp_ns)
);

CREATE TABLE IF NOT EXISTS pagebridge_audit_batches (
    workspace_id      VARCHAR(64)  NOT NULL,
    batch_id          BIGINT       NOT NULL,
    first_event_id    VARCHAR(32)  NOT NULL,
    last_event_id     VARCHAR(32)  NOT NULL,
    leaf_count        INT          NOT NULL,
    root              VARBINARY(32) NOT NULL,
    anchored_log      VARCHAR(128),
    anchored_index    BIGINT,
    PRIMARY KEY (workspace_id, batch_id)
);
"#;

/// SQL Server DDL.

pub const MSSQL_DDL: &str = r#"
IF NOT EXISTS (SELECT 1 FROM sys.tables WHERE name = N'pagebridge_audit')
BEGIN
    CREATE TABLE pagebridge_audit (
        workspace_id   NVARCHAR(64)  NOT NULL,
        event_id       NVARCHAR(32)  NOT NULL,
        timestamp_ns   BIGINT        NOT NULL,
        action         NVARCHAR(32)  NOT NULL,
        outcome        NVARCHAR(32)  NOT NULL,
        adapter        NVARCHAR(64)  NOT NULL,
        llm_provider   NVARCHAR(64),
        llm_model      NVARCHAR(128),
        input_tokens   INT           NOT NULL DEFAULT 0,
        output_tokens  INT           NOT NULL DEFAULT 0,
        latency_ms     INT           NOT NULL DEFAULT 0,
        sensitivity    NVARCHAR(64),
        parent_event   NVARCHAR(32),
        prev_hash      VARBINARY(32) NOT NULL,
        event_hash     VARBINARY(32) NOT NULL,
        signature      VARBINARY(64) NOT NULL,
        key_id         NVARCHAR(64)  NOT NULL,
        body           NVARCHAR(MAX) NOT NULL,
        CONSTRAINT PK_pagebridge_audit PRIMARY KEY (workspace_id, event_id)
    );
    CREATE INDEX pagebridge_audit_ts_idx ON pagebridge_audit (workspace_id, timestamp_ns);
END;
"#;

/// Oracle DDL.

pub const ORACLE_DDL: &str = r#"
BEGIN
EXECUTE IMMEDIATE '
CREATE TABLE pagebridge_audit (
    workspace_id   VARCHAR2(64)   NOT NULL,
    event_id       VARCHAR2(32)   NOT NULL,
    timestamp_ns   NUMBER         NOT NULL,
    action         VARCHAR2(32)   NOT NULL,
    outcome        VARCHAR2(32)   NOT NULL,
    adapter        VARCHAR2(64)   NOT NULL,
    llm_provider   VARCHAR2(64),
    llm_model      VARCHAR2(128),
    input_tokens   NUMBER         DEFAULT 0,
    output_tokens  NUMBER         DEFAULT 0,
    latency_ms     NUMBER         DEFAULT 0,
    sensitivity    VARCHAR2(64),
    parent_event   VARCHAR2(32),
    prev_hash      RAW(32)        NOT NULL,
    event_hash     RAW(32)        NOT NULL,
    signature      RAW(64)        NOT NULL,
    key_id         VARCHAR2(64)   NOT NULL,
    body           CLOB           NOT NULL,
    CONSTRAINT pk_pagebridge_audit PRIMARY KEY (workspace_id, event_id)
)';
EXCEPTION WHEN OTHERS THEN IF SQLCODE != -955 THEN RAISE; END IF;
END;
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ddls_are_nonempty_and_mention_table() {
        for ddl in [POSTGRES_DDL, SQLITE_DDL, MYSQL_DDL, MSSQL_DDL, ORACLE_DDL] {
            assert!(ddl.contains("pagebridge_audit"));
        }
    }
}
