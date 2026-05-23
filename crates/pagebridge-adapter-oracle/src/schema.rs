//! Oracle schema bootstrap, idempotent. Oracle's DDL does not have
//! `CREATE TABLE IF NOT EXISTS`, so each statement is wrapped in a PL/SQL
//! block that swallows ORA-00955 (object already exists).

use crate::err;
use crate::pool::OraclePool;
use pagebridge_core::error::Result;

pub async fn create(pool: &OraclePool) -> Result<()> {
    pool.with_conn(|conn| {
        for stmt in STATEMENTS {
            conn.execute(stmt, &[]).map_err(|e| err("migrate", e))?;
        }
        conn.commit().map_err(|e| err("commit", e))?;
        Ok(())
    })
    .await
}

const STATEMENTS: &[&str] = &[
    "BEGIN EXECUTE IMMEDIATE '
        CREATE TABLE pagebridge_nodes (
          node_id VARCHAR2(512) PRIMARY KEY,
          doc_id VARCHAR2(128) NOT NULL,
          parent_id VARCHAR2(512),
          title VARCHAR2(4000) NOT NULL,
          node_level NUMBER(3) NOT NULL,
          routing_summary VARCHAR2(4000) NOT NULL,
          summary CLOB NOT NULL,
          child_ids CLOB NOT NULL,
          span_start NUMBER(19), span_end NUMBER(19),
          page_start NUMBER(10), page_end NUMBER(10),
          keywords CLOB NOT NULL,
          is_leaf NUMBER(1) NOT NULL,
          created_at NUMBER(19) NOT NULL,
          updated_at NUMBER(19) NOT NULL,
          source_hash RAW(32) NOT NULL
        )';
     EXCEPTION WHEN OTHERS THEN IF SQLCODE != -955 THEN RAISE; END IF; END;",
    "BEGIN EXECUTE IMMEDIATE 'CREATE INDEX idx_pb_nodes_doc ON pagebridge_nodes(doc_id)';
     EXCEPTION WHEN OTHERS THEN IF SQLCODE != -955 AND SQLCODE != -1408 THEN RAISE; END IF; END;",
    "BEGIN EXECUTE IMMEDIATE 'CREATE INDEX idx_pb_nodes_parent ON pagebridge_nodes(parent_id)';
     EXCEPTION WHEN OTHERS THEN IF SQLCODE != -955 AND SQLCODE != -1408 THEN RAISE; END IF; END;",
    "BEGIN EXECUTE IMMEDIATE '
        CREATE TABLE pagebridge_docs (
          doc_id VARCHAR2(128) PRIMARY KEY,
          title VARCHAR2(4000) NOT NULL,
          source_kind VARCHAR2(64) NOT NULL,
          ingested_at NUMBER(19) NOT NULL,
          root_node_id VARCHAR2(512) NOT NULL,
          leaf_count NUMBER(10) NOT NULL,
          byte_count NUMBER(19) NOT NULL
        )';
     EXCEPTION WHEN OTHERS THEN IF SQLCODE != -955 THEN RAISE; END IF; END;",
    "BEGIN EXECUTE IMMEDIATE '
        CREATE TABLE pagebridge_raw (
          doc_id VARCHAR2(128) NOT NULL,
          offset_start NUMBER(19) NOT NULL,
          length_bytes NUMBER(10) NOT NULL,
          data BLOB NOT NULL,
          CONSTRAINT pk_pagebridge_raw PRIMARY KEY (doc_id, offset_start)
        )';
     EXCEPTION WHEN OTHERS THEN IF SQLCODE != -955 THEN RAISE; END IF; END;",
    "BEGIN EXECUTE IMMEDIATE '
        CREATE TABLE pagebridge_summary_cache (
          source_hash RAW(32) PRIMARY KEY,
          entry BLOB NOT NULL
        )';
     EXCEPTION WHEN OTHERS THEN IF SQLCODE != -955 THEN RAISE; END IF; END;",
    // Best-effort Oracle Text index. Swallow errors so migrate works on
    // editions or databases without Oracle Text installed.
    "BEGIN EXECUTE IMMEDIATE '
        CREATE INDEX idx_pb_nodes_search ON pagebridge_nodes(summary)
          INDEXTYPE IS CTXSYS.CONTEXT';
     EXCEPTION WHEN OTHERS THEN NULL; END;",
];
