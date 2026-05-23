//! Hand-rolled connection pool around the sync `oracle::Connection`.
//!
//! `bb8`-style pools assume async drivers. The `oracle` crate's driver is
//! sync, so we keep a small `parking_lot::Mutex<Vec<Connection>>` and lend
//! connections via `spawn_blocking`.

use std::sync::Arc;

use oracle::Connection;
use parking_lot::Mutex;

use crate::err;
use pagebridge_core::error::Result;

#[derive(Clone)]
pub struct OraclePool {
    inner: Arc<Inner>,
}

struct Inner {
    username: String,
    password: String,
    connect_string: String,
    pool: Mutex<Vec<Connection>>,
    max_size: usize,
}

impl OraclePool {
    pub fn new(
        username: &str,
        password: &str,
        connect_string: &str,
        max_size: usize,
    ) -> Result<Self> {
        // Validate the credentials by opening at least one connection eagerly.
        let conn = Connection::connect(username, password, connect_string)
            .map_err(|e| err("connect", e))?;
        Ok(Self {
            inner: Arc::new(Inner {
                username: username.to_owned(),
                password: password.to_owned(),
                connect_string: connect_string.to_owned(),
                pool: Mutex::new(vec![conn]),
                max_size,
            }),
        })
    }

    /// Run a closure with a borrowed connection on a blocking thread.
    pub async fn with_conn<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    {
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = inner.acquire()?;
            let res = f(&mut conn);
            inner.release(conn);
            res
        })
        .await
        .map_err(|e| err("spawn_blocking", e))?
    }
}

impl Inner {
    fn acquire(&self) -> Result<Connection> {
        if let Some(c) = self.pool.lock().pop() {
            return Ok(c);
        }
        Connection::connect(&self.username, &self.password, &self.connect_string)
            .map_err(|e| err("acquire", e))
    }

    fn release(&self, conn: Connection) {
        let mut guard = self.pool.lock();
        if guard.len() < self.max_size {
            guard.push(conn);
        }
    }
}
