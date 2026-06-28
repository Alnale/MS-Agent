//! FNV-1a 64-bit hash utility for deterministic, cross-platform hashing.

const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

/// Compute FNV-1a 64-bit hash from multiple byte slices.
pub fn fnv1a_hash(parts: &[&[u8]]) -> u64 {
    let mut hash: u64 = FNV_OFFSET_BASIS;
    for part in parts {
        for byte in *part {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    }
    hash
}

/// Compute FNV-1a hash from multiple strings and return as a hex string.
pub fn fnv1a_hash_str(parts: &[&str]) -> String {
    let byte_slices: Vec<&[u8]> = parts.iter().map(|s| s.as_bytes()).collect();
    format!("{:016x}", fnv1a_hash(&byte_slices))
}

/// Compute cosine similarity between two vectors.
/// Returns 0.0 if either vector is zero-length or if vectors have different lengths.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}
