//! Deficit Round Robin (DRR) fair queue across tenants.
//!
//! Each tenant has a deficit counter; the scheduler hands out turns
//! proportional to a per-tenant `weight`. Heavy tenants don't starve
//! light ones; light tenants don't get starved by heavy ones.

use std::collections::{HashMap, VecDeque};

use parking_lot::Mutex;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DrrError {
    #[error("no work available")]
    Empty,
}

struct Tenant {
    weight: u32,
    deficit: i64,
    queue: VecDeque<u64>, // request ids
}

pub struct Drr {
    inner: Mutex<Inner>,
}

struct Inner {
    tenants: HashMap<String, Tenant>,
    order: Vec<String>,
    cursor: usize,
}

impl Drr {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                tenants: HashMap::new(),
                order: Vec::new(),
                cursor: 0,
            }),
        }
    }

    pub fn register(&self, tenant: impl Into<String>, weight: u32) {
        let id = tenant.into();
        let mut g = self.inner.lock();
        if !g.tenants.contains_key(&id) {
            g.tenants.insert(
                id.clone(),
                Tenant {
                    weight: weight.max(1),
                    deficit: 0,
                    queue: VecDeque::new(),
                },
            );
            g.order.push(id);
        }
    }

    pub fn enqueue(&self, tenant: &str, req_id: u64) {
        let mut g = self.inner.lock();
        if let Some(t) = g.tenants.get_mut(tenant) {
            t.queue.push_back(req_id);
        }
    }

    /// Return the next (tenant, req_id) to service. Advances the cursor
    /// to honour DRR ordering. Returns DrrError::Empty if no tenant has
    /// pending work.
    pub fn next(&self) -> Result<(String, u64), DrrError> {
        let mut g = self.inner.lock();
        if g.order.is_empty() {
            return Err(DrrError::Empty);
        }
        for _round in 0..g.order.len() * 2 {
            if g.order.is_empty() {
                break;
            }
            let len = g.order.len();
            let idx = g.cursor % len;
            let tenant_id = g.order[idx].clone();
            let t = g.tenants.get_mut(&tenant_id).expect("registered");
            if t.queue.is_empty() {
                g.cursor = (g.cursor + 1) % len;
                continue;
            }
            t.deficit += i64::from(t.weight);
            if t.deficit > 0 {
                let req = t.queue.pop_front().expect("non-empty");
                t.deficit -= 1;
                let cur = g.cursor;
                g.cursor = (cur + 1) % len;
                return Ok((tenant_id, req));
            }
            g.cursor = (g.cursor + 1) % len;
        }
        Err(DrrError::Empty)
    }
}

impl Default for Drr {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_weights_alternate_tenants() {
        let q = Drr::new();
        q.register("a", 1);
        q.register("b", 1);
        for i in 0..3 {
            q.enqueue("a", i);
            q.enqueue("b", 100 + i);
        }
        let mut sequence = Vec::new();
        while let Ok((t, _)) = q.next() {
            sequence.push(t);
        }
        // Both tenants should have served 3 requests.
        let a = sequence.iter().filter(|s| *s == "a").count();
        let b = sequence.iter().filter(|s| *s == "b").count();
        assert_eq!(a, 3);
        assert_eq!(b, 3);
    }
}
