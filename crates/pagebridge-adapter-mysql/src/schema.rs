//! MySQL/MariaDB schema bootstrap. Idempotent.

use mysql_async::prelude::*;
use mysql_async::Pool;

use pagebridge_core::error::Result;

pub async fn create(pool: &Pool) -> Result<()> {
    let mut conn = pool
        .get_conn()
        .await
        .map_err(|e| crate::err("migrate connect", e))?;
    for stmt in STATEMENTS {
        stmt.ignore(&mut conn)
            .await
            .map_err(|e| crate::err("migrate", e))?;
    }
    Ok(())
}

const STATEMENTS: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS pagebridge_nodes (
        node_id VARCHAR(512) NOT NULL PRIMARY KEY,
        doc_id VARCHAR(128) NOT NULL,
        parent_id VARCHAR(512) NULL,
        title TEXT NOT NULL,
        level TINYINT UNSIGNED NOT NULL,
        routing_summary TEXT NOT NULL,
        summary MEDIUMTEXT NOT NULL,
        child_ids JSON NOT NULL,
        span_start BIGINT NULL, span_end BIGINT NULL,
        page_start INT NULL, page_end INT NULL,
        keywords JSON NOT NULL,
        is_leaf TINYINT(1) NOT NULL,
        created_at BIGINT NOT NULL,
        updated_at BIGINT NOT NULL,
        source_hash VARBINARY(32) NOT NULL,
        INDEX idx_pb_nodes_doc (doc_id),
        INDEX idx_pb_nodes_parent (parent_id),
        FULLTEXT KEY ft_pb_nodes (title, routing_summary, summary)
    ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci",
    "CREATE TABLE IF NOT EXISTS pagebridge_docs (
        doc_id VARCHAR(128) NOT NULL PRIMARY KEY,
        title TEXT NOT NULL,
        source_kind VARCHAR(64) NOT NULL,
        ingested_at BIGINT NOT NULL,
        root_node_id VARCHAR(512) NOT NULL,
        leaf_count INT NOT NULL,
        byte_count BIGINT NOT NULL
    ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci",
    "CREATE TABLE IF NOT EXISTS pagebridge_raw (
        doc_id VARCHAR(128) NOT NULL,
        offset_start BIGINT NOT NULL,
        length INT NOT NULL,
        data MEDIUMBLOB NOT NULL,
        PRIMARY KEY (doc_id, offset_start)
    ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci",
    "CREATE TABLE IF NOT EXISTS pagebridge_summary_cache (
        source_hash VARBINARY(32) NOT NULL PRIMARY KEY,
        entry MEDIUMBLOB NOT NULL
    ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci",
];
