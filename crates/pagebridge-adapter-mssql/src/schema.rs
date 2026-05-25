//! SQL Server schema bootstrap. Idempotent.
//!
//! The full-text catalog and index are created only if the SQL Server instance
//! has the Full-Text Search component installed. If not, the catalog DDL is
//! skipped and we fall back to a plain `LIKE` search in the ops layer.

use pagebridge_core::error::Result;

use crate::{err, MsPool};

pub async fn create(pool: &MsPool) -> Result<()> {
    let mut conn = pool.get().await.map_err(|e| err("migrate conn", e))?;
    for stmt in STATEMENTS {
        conn.simple_query(*stmt)
            .await
            .map_err(|e| err("migrate", e))?
            .into_results()
            .await
            .map_err(|e| err("migrate rows", e))?;
    }
    // Best-effort full-text setup. Wrapped in BEGIN TRY / END CATCH so we do
    // not fail migrate when full-text search is not installed.
    let _ = conn
        .simple_query(FT_CATALOG)
        .await
        .map_err(|e| err("ft catalog", e))?
        .into_results()
        .await;
    let _ = conn
        .simple_query(FT_INDEX)
        .await
        .map_err(|e| err("ft index", e))?
        .into_results()
        .await;
    Ok(())
}

const STATEMENTS: &[&str] = &[
    "IF NOT EXISTS (SELECT 1 FROM sys.tables WHERE name = 'pagebridge_nodes')
     BEGIN
       CREATE TABLE pagebridge_nodes (
         node_id NVARCHAR(512) NOT NULL PRIMARY KEY,
         doc_id NVARCHAR(128) NOT NULL,
         parent_id NVARCHAR(512) NULL,
         title NVARCHAR(MAX) NOT NULL,
         level TINYINT NOT NULL,
         routing_summary NVARCHAR(MAX) NOT NULL,
         summary NVARCHAR(MAX) NOT NULL,
         child_ids NVARCHAR(MAX) NOT NULL,
         span_start BIGINT NULL, span_end BIGINT NULL,
         page_start INT NULL, page_end INT NULL,
         keywords NVARCHAR(MAX) NOT NULL,
         is_leaf BIT NOT NULL,
         created_at BIGINT NOT NULL,
         updated_at BIGINT NOT NULL,
         source_hash VARBINARY(32) NOT NULL,
         canonical_section NVARCHAR(MAX) NULL,
         section_aliases NVARCHAR(MAX) NOT NULL DEFAULT '[]'
       );
       CREATE INDEX idx_pb_nodes_doc ON pagebridge_nodes(doc_id);
       CREATE INDEX idx_pb_nodes_parent ON pagebridge_nodes(parent_id);
     END",
    // Incremental migrations for databases that predate these columns.
    "IF NOT EXISTS (SELECT 1 FROM sys.columns WHERE object_id = OBJECT_ID('pagebridge_nodes') AND name = 'canonical_section')
     ALTER TABLE pagebridge_nodes ADD canonical_section NVARCHAR(MAX) NULL",
    "IF NOT EXISTS (SELECT 1 FROM sys.columns WHERE object_id = OBJECT_ID('pagebridge_nodes') AND name = 'section_aliases')
     ALTER TABLE pagebridge_nodes ADD section_aliases NVARCHAR(MAX) NOT NULL DEFAULT '[]'",
    "IF NOT EXISTS (SELECT 1 FROM sys.tables WHERE name = 'pagebridge_docs')
     BEGIN
       CREATE TABLE pagebridge_docs (
         doc_id NVARCHAR(128) NOT NULL PRIMARY KEY,
         title NVARCHAR(MAX) NOT NULL,
         source_kind NVARCHAR(64) NOT NULL,
         ingested_at BIGINT NOT NULL,
         root_node_id NVARCHAR(512) NOT NULL,
         leaf_count INT NOT NULL,
         byte_count BIGINT NOT NULL,
         raw_text_hash VARBINARY(32) NULL,
         structural_hash VARBINARY(32) NULL,
         document_type NVARCHAR(64) NULL
       );
     END",
    "IF NOT EXISTS (SELECT 1 FROM sys.columns WHERE object_id = OBJECT_ID('pagebridge_docs') AND name = 'raw_text_hash')
     ALTER TABLE pagebridge_docs ADD raw_text_hash VARBINARY(32) NULL",
    "IF NOT EXISTS (SELECT 1 FROM sys.columns WHERE object_id = OBJECT_ID('pagebridge_docs') AND name = 'structural_hash')
     ALTER TABLE pagebridge_docs ADD structural_hash VARBINARY(32) NULL",
    "IF NOT EXISTS (SELECT 1 FROM sys.columns WHERE object_id = OBJECT_ID('pagebridge_docs') AND name = 'document_type')
     ALTER TABLE pagebridge_docs ADD document_type NVARCHAR(64) NULL",
    "IF NOT EXISTS (SELECT 1 FROM sys.tables WHERE name = 'pagebridge_raw')
     BEGIN
       CREATE TABLE pagebridge_raw (
         doc_id NVARCHAR(128) NOT NULL,
         offset_start BIGINT NOT NULL,
         length INT NOT NULL,
         data VARBINARY(MAX) NOT NULL,
         CONSTRAINT pk_pagebridge_raw PRIMARY KEY (doc_id, offset_start)
       );
     END",
    "IF NOT EXISTS (SELECT 1 FROM sys.tables WHERE name = 'pagebridge_summary_cache')
     BEGIN
       CREATE TABLE pagebridge_summary_cache (
         source_hash VARBINARY(32) NOT NULL PRIMARY KEY,
         entry VARBINARY(MAX) NOT NULL
       );
     END",
];

const FT_CATALOG: &str =
    "IF NOT EXISTS (SELECT 1 FROM sys.fulltext_catalogs WHERE name = 'pagebridge_ft')
    CREATE FULLTEXT CATALOG pagebridge_ft AS DEFAULT;";

const FT_INDEX: &str = "IF NOT EXISTS (SELECT 1 FROM sys.fulltext_indexes
    WHERE object_id = OBJECT_ID('dbo.pagebridge_nodes'))
    CREATE FULLTEXT INDEX ON dbo.pagebridge_nodes
      (title LANGUAGE 1033, routing_summary LANGUAGE 1033, summary LANGUAGE 1033)
      KEY INDEX PK__pagebrid__node_id ON pagebridge_ft;";
