//! Pluggable source of new configuration values. Filesystem source is
//! provided; etcd / Consul backends live in separate crates.

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;

#[async_trait]
pub trait ConfigSource<T>: Send + Sync + 'static
where
    T: Send + Sync + 'static,
{
    /// Block until a new value is available or the source closes.
    async fn next(&self) -> Option<T>;
}

pub struct FileSource<T> {
    path: PathBuf,
    poll: Duration,
    last_mtime: tokio::sync::Mutex<Option<std::time::SystemTime>>,
    parse: fn(&[u8]) -> Option<T>,
}

impl<T: Send + Sync + 'static> FileSource<T> {
    pub fn new(
        path: impl Into<PathBuf>,
        poll: Duration,
        parse: fn(&[u8]) -> Option<T>,
    ) -> Self {
        Self {
            path: path.into(),
            poll,
            last_mtime: tokio::sync::Mutex::new(None),
            parse,
        }
    }
}

#[async_trait]
impl<T: Send + Sync + 'static> ConfigSource<T> for FileSource<T> {
    async fn next(&self) -> Option<T> {
        loop {
            tokio::time::sleep(self.poll).await;
            let meta = tokio::fs::metadata(&self.path).await.ok()?;
            let mtime = meta.modified().ok();
            let mut last = self.last_mtime.lock().await;
            if *last != mtime {
                *last = mtime;
                let bytes = tokio::fs::read(&self.path).await.ok()?;
                if let Some(v) = (self.parse)(&bytes) {
                    return Some(v);
                }
            }
        }
    }
}
