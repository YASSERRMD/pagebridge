//! Merkle batch anchoring.
//!
//! Every N events the writer aggregates the per-event hashes into a binary
//! Merkle tree and publishes the root. The root is what gets pushed to an
//! integrity store (Trillian, an S3 object-lock bucket, an internal table).
//! Inclusion proofs let any consumer prove "event X appears in batch B
//! whose root is R" without retrieving every event in the batch.
//!
//! We use a straightforward sha256-based binary tree. Odd nodes at any
//! level are duplicated (the "Bitcoin" convention) so the tree is always
//! perfectly balanced.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{AuditError, Result};

/// A Merkle root plus the leaf range it covers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MerkleBatch {
    pub batch_id: u64,
    pub workspace_id: String,
    pub first_event_id: String,
    pub last_event_id: String,
    pub leaf_count: u32,
    #[serde(with = "crate::merkle::hex_array")]
    pub root: [u8; 32],
}

/// A path of sibling hashes from a leaf to the Merkle root.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InclusionProof {
    pub leaf_index: u32,
    pub leaf_count: u32,
    pub siblings: Vec<ProofNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofNode {
    /// "L" if the sibling sits to the left of the running hash, "R" otherwise.
    pub side: char,
    #[serde(with = "crate::merkle::hex_array")]
    pub hash: [u8; 32],
}

/// Build the Merkle root over an ordered slice of leaf hashes.
#[must_use]
pub fn merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.is_empty() {
        return [0u8; 32];
    }
    let mut layer: Vec<[u8; 32]> = leaves.to_vec();
    while layer.len() > 1 {
        if layer.len() % 2 == 1 {
            layer.push(*layer.last().unwrap());
        }
        let mut next = Vec::with_capacity(layer.len() / 2);
        for pair in layer.chunks(2) {
            next.push(hash_pair(&pair[0], &pair[1]));
        }
        layer = next;
    }
    layer[0]
}

/// Build the inclusion proof for `leaf_index` over `leaves`.
pub fn merkle_proof(leaves: &[[u8; 32]], leaf_index: usize) -> Result<InclusionProof> {
    if leaves.is_empty() || leaf_index >= leaves.len() {
        return Err(AuditError::Internal(format!(
            "leaf_index {leaf_index} out of range for {} leaves",
            leaves.len()
        )));
    }
    let leaf_count = leaves.len();
    let mut siblings = Vec::new();
    let mut layer: Vec<[u8; 32]> = leaves.to_vec();
    let mut idx = leaf_index;
    while layer.len() > 1 {
        if layer.len() % 2 == 1 {
            layer.push(*layer.last().unwrap());
        }
        let sibling_idx = idx ^ 1;
        let sibling = layer[sibling_idx];
        let side = if sibling_idx > idx { 'R' } else { 'L' };
        siblings.push(ProofNode {
            side,
            hash: sibling,
        });
        let mut next = Vec::with_capacity(layer.len() / 2);
        for pair in layer.chunks(2) {
            next.push(hash_pair(&pair[0], &pair[1]));
        }
        layer = next;
        idx /= 2;
    }
    Ok(InclusionProof {
        leaf_index: u32::try_from(leaf_index).unwrap_or(u32::MAX),
        leaf_count: u32::try_from(leaf_count).unwrap_or(u32::MAX),
        siblings,
    })
}

/// Walk an inclusion proof and reconstruct the expected root.
pub fn verify_inclusion(proof: &InclusionProof, leaf: [u8; 32], expected_root: [u8; 32]) -> bool {
    let mut running = leaf;
    for node in &proof.siblings {
        running = match node.side {
            'L' => hash_pair(&node.hash, &running),
            _ => hash_pair(&running, &node.hash),
        };
    }
    running == expected_root
}

fn hash_pair(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(a);
    h.update(b);
    let out = h.finalize();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out);
    arr
}

mod hex_array {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let s = String::deserialize(d)?;
        let v = hex::decode(&s).map_err(serde::de::Error::custom)?;
        if v.len() != 32 {
            return Err(serde::de::Error::custom("expected 32 hex bytes"));
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&v);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(seed: u8) -> [u8; 32] {
        let mut a = [0u8; 32];
        a[0] = seed;
        a
    }

    #[test]
    fn root_of_one_leaf_is_itself() {
        let l = leaf(7);
        assert_eq!(merkle_root(&[l]), l);
    }

    #[test]
    fn proof_round_trip_even() {
        let leaves: Vec<_> = (0..4).map(leaf).collect();
        let root = merkle_root(&leaves);
        for i in 0..leaves.len() {
            let proof = merkle_proof(&leaves, i).unwrap();
            assert!(verify_inclusion(&proof, leaves[i], root));
        }
    }

    #[test]
    fn proof_round_trip_odd() {
        let leaves: Vec<_> = (0..5).map(leaf).collect();
        let root = merkle_root(&leaves);
        for i in 0..leaves.len() {
            let proof = merkle_proof(&leaves, i).unwrap();
            assert!(verify_inclusion(&proof, leaves[i], root));
        }
    }

    #[test]
    fn tampered_leaf_breaks_proof() {
        let leaves: Vec<_> = (0..4).map(leaf).collect();
        let root = merkle_root(&leaves);
        let proof = merkle_proof(&leaves, 1).unwrap();
        let tampered = leaf(99);
        assert!(!verify_inclusion(&proof, tampered, root));
    }
}
