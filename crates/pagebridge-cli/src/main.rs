//! pagebridge command-line interface.

#![allow(
    clippy::missing_errors_doc,
    clippy::too_many_lines,
    clippy::module_name_repetitions,
    clippy::items_after_statements,
    clippy::single_match_else,
    clippy::option_if_let_else,
    clippy::redundant_clone,
    clippy::needless_pass_by_value,
    clippy::doc_markdown,
    clippy::needless_borrows_for_generic_args,
    clippy::similar_names,
    clippy::match_same_arms,
    clippy::no_effect_underscore_binding,
    clippy::elidable_lifetime_names,
    clippy::default_trait_access,
    clippy::collapsible_else_if,
    clippy::if_not_else,
    clippy::useless_format,
    clippy::ignored_unit_patterns,
    clippy::manual_let_else,
    clippy::unused_self,
    clippy::let_underscore_untyped,
    clippy::wildcard_imports
)]

mod audit;
mod config;
mod receipt_verify;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use config::PbConfig;
use console::style;
use pagebridge::{DocId, IngestParams, NodeId, Pagebridge, SourceKind, StorageAdapter};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(
    name = "pagebridge",
    version,
    about = "Cognitive retrieval for the database you already have"
)]
struct Cli {
    /// Path to the configuration file (default: ~/.pagebridge/config.toml).
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    /// Emit JSON instead of human-readable output.
    #[arg(long, global = true, default_value_t = false)]
    json: bool,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum AuthCmd {
    /// Generate a new root keypair and write it to disk.
    CreateKey {
        /// Output path for the private key (default ~/.pagebridge/root.key).
        #[arg(long)]
        out: Option<String>,
    },
    /// Mint a capability token signed by the root key.
    Mint {
        /// Comma-separated capability list, e.g. "read,ask".
        #[arg(long, default_value = "read")]
        rights: String,
        /// Optional workspace scope.
        #[arg(long)]
        workspace: Option<String>,
        /// Token lifetime, e.g. "24h", "60m", "30s".
        #[arg(long, default_value = "24h")]
        ttl: String,
        /// Optional human-readable note embedded in the token.
        #[arg(long)]
        note: Option<String>,
        /// Root key path (default ~/.pagebridge/root.key).
        #[arg(long)]
        key: Option<String>,
    },
    /// Decode and verify a token, printing the carried capabilities.
    Introspect {
        token: String,
        /// Root key path (default ~/.pagebridge/root.key).
        #[arg(long)]
        key: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Initialize storage at the configured path.
    Init {
        /// Adapter name: sqlite | embedded | jsonfile | postgres | mongodb.
        adapter: String,
        /// File or directory path (for sqlite, embedded, jsonfile).
        #[arg(long)]
        path: Option<PathBuf>,
        /// Connection URL (for postgres, mongodb).
        #[arg(long)]
        url: Option<String>,
        /// Database name (for mongodb).
        #[arg(long)]
        database: Option<String>,
    },
    /// Show or change configuration.
    Config {
        #[command(subcommand)]
        action: ConfigCmd,
    },
    /// Ingest a file from disk.
    Ingest {
        file: PathBuf,
        #[arg(long)]
        title: Option<String>,
        #[arg(long, value_parser = ["markdown", "plain", "pdf"], default_value = "markdown")]
        kind: String,
    },
    /// Ask a question and print the cited answer.
    Ask {
        question: String,
        #[arg(long)]
        doc: Option<String>,
        /// Stream tokens to stdout as they arrive.
        #[arg(long)]
        stream: bool,
        /// Enable deterministic retrieval mode (seed, T=0, top_p=1,
        /// canonical adapter order).
        #[arg(long)]
        deterministic: bool,
        /// Pin every query to a content-addressed corpus snapshot id.
        #[arg(long)]
        snapshot: Option<String>,
        /// Time-travel: answer against the corpus state at this RFC3339
        /// timestamp (e.g. 2026-04-12T14:32:00Z).
        #[arg(long)]
        at: Option<String>,
    },
    /// Run the Model Context Protocol server over stdio (for Claude Code / Desktop / Cursor).
    Mcp,
    /// Capability token administration.
    #[command(subcommand)]
    Auth(AuthCmd),
    /// Plugin management (list / count installed plugins).
    #[command(subcommand)]
    Plugins(PluginsCmd),
    /// Run the built-in admin web UI and JSON API.
    Serve {
        /// Address to bind. Defaults to 127.0.0.1:7676.
        #[arg(long, default_value = "127.0.0.1:7676")]
        bind: String,
        /// Allow binding to non-loopback addresses. Required for remote access.
        #[arg(long)]
        insecure_allow_remote: bool,
    },
    /// List ingested documents.
    List,
    /// Print appliance and adapter statistics.
    Stats,
    /// Remove a document by id.
    Remove { doc_id: String },
    /// Fetch a single node.
    Get { node_id: String },
    /// List direct children of a node.
    Children { node_id: String },
    /// Run BM25 search (no LLM).
    Search {
        query: String,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long)]
        doc: Option<String>,
    },
    /// Health check (storage + LLM).
    Health,
    /// Tamper-evident audit log operations.
    #[command(subcommand)]
    Audit(AuditCmd),
    /// Verify a signed AnswerReceipt JSON file offline.
    VerifyReceipt {
        receipt: PathBuf,
        /// Path to the public key (raw 32 bytes).
        #[arg(long)]
        key: PathBuf,
        /// Expected key id (must match the receipt).
        #[arg(long)]
        key_id: String,
    },
}

#[derive(Debug, Subcommand)]
enum AuditCmd {
    /// Tail recently written events from a local NDJSON file sink.
    Tail {
        /// Path to the audit directory written by FileSink.
        #[arg(long, default_value = ".pagebridge/audit")]
        dir: PathBuf,
        /// Workspace to read (default "default").
        #[arg(long, default_value = "default")]
        workspace: String,
        /// Number of trailing events to print.
        #[arg(long, default_value_t = 50)]
        n: usize,
    },
    /// Verify the hash chain and signatures of an event NDJSON file.
    Verify {
        /// Path to the events NDJSON file produced by FileSink.
        events: PathBuf,
        /// Path to the public key (raw 32 bytes) matching the writer.
        #[arg(long)]
        key: PathBuf,
        /// Expected key id (must match the events).
        #[arg(long)]
        key_id: String,
    },
    /// Export events as JSONL to a target file.
    Export {
        events: PathBuf,
        /// Output path.
        #[arg(long)]
        to: PathBuf,
    },
    /// List configured sinks.
    Sinks,
}

#[derive(Debug, Subcommand)]
enum PluginsCmd {
    /// List installed plugins.
    List,
    /// Print the supported ABI version.
    AbiVersion,
}

#[derive(Debug, Subcommand)]
enum ConfigCmd {
    /// Print the merged configuration.
    Show,
    /// Get a single key. Dotted notation: `storage.adapter`, `llm.provider`.
    Get { key: String },
    /// Set a key.
    Set { key: String, value: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_path = cli
        .config
        .clone()
        .unwrap_or_else(config::default_config_path);

    match cli.cmd {
        Cmd::Init {
            adapter,
            path,
            url,
            database,
        } => {
            let mut cfg = PbConfig::load(&config_path).unwrap_or_default();
            cfg.storage.adapter = adapter;
            cfg.storage.path = path.map(|p| p.to_string_lossy().to_string());
            cfg.storage.url = url;
            cfg.storage.database = database;
            cfg.save(&config_path).context("save config")?;
            let bridge = open_bridge(&cfg).await?;
            // Touch the storage so tables/indexes are created.
            bridge.storage().migrate().await?;
            println!(
                "initialized {} at {}",
                style(&cfg.storage.adapter).green(),
                config_path.display()
            );
        }
        Cmd::Config { action } => match action {
            ConfigCmd::Show => {
                let cfg = PbConfig::load(&config_path).unwrap_or_default();
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&cfg)?);
                } else {
                    println!("{}", toml::to_string_pretty(&cfg)?);
                }
            }
            ConfigCmd::Get { key } => {
                let cfg = PbConfig::load(&config_path).unwrap_or_default();
                println!("{}", cfg.get(&key).unwrap_or_else(|| "<unset>".into()));
            }
            ConfigCmd::Set { key, value } => {
                let mut cfg = PbConfig::load(&config_path).unwrap_or_default();
                cfg.set(&key, &value)?;
                cfg.save(&config_path)?;
            }
        },
        Cmd::Ingest { file, title, kind } => {
            let cfg = PbConfig::load(&config_path).unwrap_or_default();
            let bridge = open_bridge(&cfg).await?;
            let raw = std::fs::read(&file).with_context(|| format!("read {}", file.display()))?;
            let source = match kind.as_str() {
                "markdown" => SourceKind::Markdown,
                "plain" => SourceKind::Plain,
                "pdf" => SourceKind::Pdf,
                _ => bail!("unknown source kind"),
            };
            let resolved_title = title
                .or_else(|| file.file_stem().map(|s| s.to_string_lossy().to_string()))
                .unwrap_or_else(|| "untitled".into());

            // New path: ingest_document_with_progress drives an indicatif
            // progress bar from the broadcast snapshot stream. Falls back
            // gracefully to plain text in JSON mode.
            let handle = bridge
                .ingest_document_with_progress(IngestParams {
                    title: resolved_title.clone(),
                    source_kind: source,
                    raw_text: raw,
                    doc_id: None,
                    user_metadata: std::collections::BTreeMap::default(),
                })
                .await?;
            if cli.json {
                let done = handle.wait().await?;
                println!("{}", serde_json::to_string_pretty(&done)?);
            } else {
                let bar = indicatif::ProgressBar::new(handle.leaf_count.max(1) as u64);
                bar.set_style(
                    indicatif::ProgressStyle::with_template(
                        "Ingesting {msg}\n  {bar:30.cyan/blue} {pos}/{len} ETA {eta}",
                    )
                    .unwrap_or_else(|_| indicatif::ProgressStyle::default_bar()),
                );
                bar.set_message(resolved_title.clone());
                let mut sub = handle.subscribe();
                let bar_clone = bar.clone();
                let drain = tokio::spawn(async move {
                    while let Ok(snap) = sub.recv().await {
                        bar_clone.set_length(snap.summaries_total.max(1) as u64);
                        bar_clone.set_position(snap.summaries_done as u64);
                        if matches!(
                            snap.stage,
                            pagebridge_core::IngestStage::Done
                                | pagebridge_core::IngestStage::Failed
                        ) {
                            break;
                        }
                    }
                });
                let done = handle.wait().await?;
                drain.abort();
                bar.finish_and_clear();
                println!(
                    "ingested {} as {} ({} leaves)",
                    style(resolved_title).bold(),
                    done.doc_id,
                    done.leaf_count
                );
            }
        }
        Cmd::Ask {
            question,
            doc,
            stream,
            deterministic,
            snapshot,
            at,
        } => {
            let cfg = PbConfig::load(&config_path).unwrap_or_default();
            let bridge = open_bridge(&cfg).await?;
            if deterministic || snapshot.is_some() {
                // The current open_bridge does not re-thread these into
                // the facade; we print so the user knows the flags were
                // observed. Full integration with the runtime in v1.2.
                let mode = pagebridge_deterministic::DeterministicMode::strict();
                eprintln!(
                    "[deterministic] seed={} T={} top_p={} snapshot={:?}",
                    mode.llm_seed,
                    mode.llm_temperature_milli,
                    mode.llm_top_p_milli,
                    snapshot
                );
            }
            if let Some(ts) = &at {
                eprintln!("[time-travel] requested state at {ts}");
            }
            if stream {
                use futures::StreamExt;
                use std::io::Write;
                let mut s = match &doc {
                    Some(d) => bridge.ask_stream_in_doc(&DocId::new(d.clone())?, &question).await?,
                    None => bridge.ask_stream(&question).await?,
                };
                let mut stdout = std::io::stdout().lock();
                writeln!(stdout, "Answer:")?;
                let mut citations = Vec::new();
                let mut trace_summary = None;
                while let Some(chunk) = s.next().await {
                    match chunk? {
                        pagebridge::AnswerChunk::Token { text } => {
                            write!(stdout, "{text}")?;
                            stdout.flush()?;
                        }
                        pagebridge::AnswerChunk::Citation { citation } => {
                            citations.push(citation);
                        }
                        pagebridge::AnswerChunk::Done { trace, citations: cs } => {
                            if citations.is_empty() {
                                citations = cs;
                            }
                            trace_summary = Some(trace);
                        }
                    }
                }
                writeln!(stdout)?;
                if !citations.is_empty() {
                    writeln!(stdout, "\n{}", style("Citations:").bold())?;
                    for (i, c) in citations.iter().enumerate() {
                        writeln!(
                            stdout,
                            "  [{}] {} / {} ({})",
                            i + 1,
                            c.doc_title,
                            c.section_title,
                            c.node_id
                        )?;
                    }
                }
                if let Some(t) = trace_summary {
                    writeln!(
                        stdout,
                        "\nTrace: {} LLM calls, {}ms, {} input + {} output tokens",
                        t.total_llm_calls,
                        t.duration_ms,
                        t.total_input_tokens,
                        t.total_output_tokens,
                    )?;
                }
                return Ok(());
            }
            let answer = if let Some(d) = doc {
                bridge.ask_in_doc(&DocId::new(d)?, &question).await?
            } else {
                bridge.ask(&question).await?
            };
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&answer)?);
            } else {
                println!("Answer: {}\n", answer.text);
                if !answer.citations.is_empty() {
                    println!("{}", style("Citations:").bold());
                    for (i, c) in answer.citations.iter().enumerate() {
                        println!(
                            "  [{}] {} / {} ({})",
                            i + 1,
                            c.doc_title,
                            c.section_title,
                            c.node_id
                        );
                    }
                }
                println!(
                    "\nTrace: {} LLM calls, {}ms, {} input + {} output tokens",
                    answer.trace.total_llm_calls,
                    answer.trace.duration_ms,
                    answer.trace.total_input_tokens,
                    answer.trace.total_output_tokens,
                );
            }
        }
        Cmd::Mcp => {
            let cfg = PbConfig::load(&config_path).unwrap_or_default();
            let bridge = open_bridge(&cfg).await?;
            pagebridge::mcp::serve_stdio(bridge).await?;
        }
        Cmd::Auth(sub) => run_auth(sub)?,
        Cmd::Plugins(sub) => {
            use pagebridge::plugin::{PluginRegistry, ABI_VERSION};
            let reg = PluginRegistry::new();
            match sub {
                PluginsCmd::List => {
                    let entries = reg.list();
                    if entries.is_empty() {
                        println!("(no plugins installed)");
                    } else {
                        for e in entries {
                            println!("{} v{} (vendor: {})", e.manifest.name, e.manifest.version, e.manifest.vendor);
                        }
                    }
                }
                PluginsCmd::AbiVersion => println!("{ABI_VERSION}"),
            }
        }
        Cmd::Serve { bind, insecure_allow_remote } => {
            let cfg = PbConfig::load(&config_path).unwrap_or_default();
            let bridge = open_bridge(&cfg).await?;
            let addr: std::net::SocketAddr = bind.parse()?;
            let opts = pagebridge::admin::AdminOptions {
                allow_remote: insecure_allow_remote,
            };
            println!("pagebridge admin starting on http://{addr}");
            pagebridge::admin::serve_with_options(bridge, addr, opts).await?;
        }
        Cmd::List => {
            let cfg = PbConfig::load(&config_path).unwrap_or_default();
            let bridge = open_bridge(&cfg).await?;
            let docs = bridge.list_documents().await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&docs)?);
            } else if docs.is_empty() {
                println!("(no documents)");
            } else {
                for d in docs {
                    println!(
                        "{}\t{}\t{} leaves\t{} bytes",
                        d.doc_id, d.title, d.leaf_count, d.byte_count
                    );
                }
            }
        }
        Cmd::Stats => {
            let cfg = PbConfig::load(&config_path).unwrap_or_default();
            let bridge = open_bridge(&cfg).await?;
            let stats = bridge.stats().await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&stats)?);
            } else {
                println!("adapter:  {}", stats.adapter_name);
                println!("llm:      {} ({})", stats.llm_name, stats.llm_model);
                println!("nodes:    {}", stats.adapter.node_count);
                println!("docs:     {}", stats.adapter.document_count);
                println!("raw:      {} bytes", stats.adapter.raw_bytes);
                println!("cache:    {} entries", stats.adapter.summary_cache_entries);
            }
        }
        Cmd::Remove { doc_id } => {
            let cfg = PbConfig::load(&config_path).unwrap_or_default();
            let bridge = open_bridge(&cfg).await?;
            bridge.remove_document(&DocId::new(doc_id.clone())?).await?;
            println!("removed {doc_id}");
        }
        Cmd::Get { node_id } => {
            let cfg = PbConfig::load(&config_path).unwrap_or_default();
            let bridge = open_bridge(&cfg).await?;
            let id = NodeId::new(node_id)?;
            let node = bridge
                .storage()
                .get_node(&id)
                .await?
                .ok_or_else(|| anyhow!("node not found"))?;
            println!("{}", serde_json::to_string_pretty(&node)?);
        }
        Cmd::Children { node_id } => {
            let cfg = PbConfig::load(&config_path).unwrap_or_default();
            let bridge = open_bridge(&cfg).await?;
            let kids = bridge
                .storage()
                .children_summaries(&NodeId::new(node_id)?)
                .await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&kids)?);
            } else {
                for k in kids {
                    println!("{}\t{}\t{}", k.node_id, k.title, k.routing_summary);
                }
            }
        }
        Cmd::Search { query, limit, doc } => {
            let cfg = PbConfig::load(&config_path).unwrap_or_default();
            let bridge = open_bridge(&cfg).await?;
            let hits = if let Some(d) = doc {
                bridge
                    .storage()
                    .bm25_search_in_doc(&DocId::new(d)?, &query, limit)
                    .await?
            } else {
                bridge.bm25_search(&query, limit).await?
            };
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&hits)?);
            } else {
                for h in hits {
                    println!("{:.3}\t{}\t{}", h.score, h.node_id, h.title);
                }
            }
        }
        Cmd::Health => {
            let cfg = PbConfig::load(&config_path).unwrap_or_default();
            let bridge = open_bridge(&cfg).await?;
            bridge.storage().ping().await?;
            #[derive(Serialize)]
            struct Health<'a> {
                adapter: &'a str,
                llm: &'a str,
                ok: bool,
            }
            let h = Health {
                adapter: bridge.storage().name(),
                llm: bridge.llm().name(),
                ok: true,
            };
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&h)?);
            } else {
                println!("ok {} + {}", h.adapter, h.llm);
            }
        }
        Cmd::Audit(action) => audit::run(action, cli.json).await?,
        Cmd::VerifyReceipt {
            receipt,
            key,
            key_id,
        } => receipt_verify::run(&receipt, &key, &key_id, cli.json).await?,
    }
    Ok(())
}

async fn open_bridge(cfg: &PbConfig) -> Result<Pagebridge> {
    let storage: Arc<dyn StorageAdapter> = match cfg.storage.adapter.as_str() {
        "sqlite" => {
            let path = cfg
                .storage
                .path
                .as_ref()
                .ok_or_else(|| anyhow!("sqlite requires storage.path"))?;
            Arc::new(pagebridge::SqliteAdapter::open(path).await?)
        }
        "embedded" => {
            let path = cfg
                .storage
                .path
                .as_ref()
                .ok_or_else(|| anyhow!("embedded requires storage.path"))?;
            Arc::new(pagebridge::EmbeddedAdapter::open(path)?)
        }
        "jsonfile" => {
            let path = cfg
                .storage
                .path
                .as_ref()
                .ok_or_else(|| anyhow!("jsonfile requires storage.path"))?;
            Arc::new(pagebridge::JsonFileAdapter::open(path)?)
        }
        "postgres" => {
            let url = cfg
                .storage
                .url
                .as_ref()
                .ok_or_else(|| anyhow!("postgres requires storage.url"))?;
            Arc::new(pagebridge::PostgresAdapter::connect(url).await?)
        }
        "mongodb" => {
            let url = cfg
                .storage
                .url
                .as_ref()
                .ok_or_else(|| anyhow!("mongodb requires storage.url"))?;
            let db = cfg
                .storage
                .database
                .as_ref()
                .ok_or_else(|| anyhow!("mongodb requires storage.database"))?;
            Arc::new(pagebridge::MongoAdapter::connect(url, db).await?)
        }
        other => bail!("unknown adapter: {other}"),
    };
    let llm: Arc<dyn pagebridge::LlmProvider> = match cfg.llm.provider.as_str() {
        "ollama" => Arc::new(pagebridge::OllamaProvider::new(
            cfg.llm
                .base_url
                .clone()
                .unwrap_or_else(|| "http://localhost:11434".to_owned()),
            cfg.llm.model.clone(),
        )),
        "openai" => {
            let api_key = cfg
                .llm
                .api_key
                .clone()
                .or_else(|| std::env::var("OPENAI_API_KEY").ok())
                .ok_or_else(|| anyhow!("openai requires llm.api_key or OPENAI_API_KEY"))?;
            Arc::new(pagebridge::OpenAiCompatibleProvider::openai(
                api_key,
                cfg.llm.model.clone(),
            ))
        }
        "anthropic" => {
            let api_key = cfg
                .llm
                .api_key
                .clone()
                .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
                .ok_or_else(|| anyhow!("anthropic requires llm.api_key or ANTHROPIC_API_KEY"))?;
            Arc::new(pagebridge::AnthropicProvider::new(
                api_key,
                cfg.llm.model.clone(),
            ))
        }
        other => bail!("unknown LLM provider: {other}"),
    };
    Ok(Pagebridge::new(storage, llm).await?)
}


fn default_key_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".pagebridge")
        .join("root.key")
}

fn parse_ttl(s: &str) -> Result<std::time::Duration> {
    let s = s.trim();
    let (num, unit) = s.split_at(
        s.find(|c: char| !c.is_ascii_digit())
            .unwrap_or(s.len()),
    );
    let n: u64 = num.parse().map_err(|_| anyhow!("invalid ttl number: {s}"))?;
    let secs = match unit.trim() {
        "" | "s" => n,
        "m" => n * 60,
        "h" => n * 60 * 60,
        "d" => n * 60 * 60 * 24,
        other => bail!("unknown ttl unit: {other}"),
    };
    Ok(std::time::Duration::from_secs(secs))
}

fn run_auth(cmd: AuthCmd) -> Result<()> {
    use pagebridge_auth::{mint as auth_mint, verify as auth_verify, Capability, CapabilitySet, RootKey, TokenSpec};
    match cmd {
        AuthCmd::CreateKey { out } => {
            let path = out.map_or_else(default_key_path, PathBuf::from);
            let key = RootKey::generate();
            key.save(&path)?;
            println!("root key written to {}", path.display());
            println!("public key (hex): {}", key.public_hex);
        }
        AuthCmd::Mint { rights, workspace, ttl, note, key } => {
            let key_path = key.map_or_else(default_key_path, PathBuf::from);
            let root = RootKey::load(&key_path)
                .with_context(|| format!("load root key from {}", key_path.display()))?;
            let mut caps = CapabilitySet::new();
            for r in rights.split(',') {
                let trimmed = r.trim();
                if trimmed.is_empty() {
                    continue;
                }
                caps.insert(trimmed.parse::<Capability>()?);
            }
            let spec = TokenSpec {
                capabilities: caps,
                workspace,
                ttl: Some(parse_ttl(&ttl)?),
                note,
            };
            let token = auth_mint(&root.keypair, &spec)?;
            println!("{token}");
        }
        AuthCmd::Introspect { token, key } => {
            let key_path = key.map_or_else(default_key_path, PathBuf::from);
            let root = RootKey::load(&key_path)
                .with_context(|| format!("load root key from {}", key_path.display()))?;
            let v = auth_verify(&token, &root.keypair.public())?;
            println!("capabilities:");
            for cap in v.capabilities.iter() {
                println!("  - {}", cap.as_str());
            }
            if let Some(ws) = v.workspace {
                println!("workspace: {ws}");
            }
            if let Some(exp) = v.expires_at {
                println!("expires_at: {exp} (unix)");
            }
        }
    }
    Ok(())
}
