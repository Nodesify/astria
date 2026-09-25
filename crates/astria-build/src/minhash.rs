// MinHash + band-LSH for near-duplicate label detection.
// Ported from upstream astria v8 _minhash.py (datasketch-compatible, fixed
// seed, no external deps): Mersenne-prime permutation family over FNV-1a
// hashed character shingles, with banded locality-sensitive hashing.

/// Mersenne prime 2^61 − 1 for the universal hash family.
const MERSENNE_PRIME: u64 = (1u64 << 61) - 1;
/// Number of MinHash permutations. With 16 bands × 8 rows this yields an
/// LSH candidate threshold of (1/16)^(1/8) ≈ 0.71 — right above the 0.7
/// similarity band the dedup verifier wants.
pub const NUM_PERMS: usize = 128;
pub const BANDS: usize = 16;
const ROWS: usize = NUM_PERMS / BANDS; // 8
const SEED: u64 = 1;

/// Deterministic (a, b) permutation parameters via SplitMix64, computed
/// once and shared by every signature call.
fn perm_params() -> &'static [(u64, u64); NUM_PERMS] {
    static PARAMS: std::sync::LazyLock<[(u64, u64); NUM_PERMS]> = std::sync::LazyLock::new(|| {
        let mut state = SEED;
        let mut next = || {
            state = state.wrapping_add(0x9E3779B97F4A7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
            z ^ (z >> 31)
        };
        let mut params = [(0u64, 0u64); NUM_PERMS];
        for p in params.iter_mut() {
            // a must be non-zero for a valid permutation of the field
            let mut a = next() % (MERSENNE_PRIME - 1) + 1;
            if a == 0 {
                a = 1;
            }
            *p = (a, next() % MERSENNE_PRIME);
        }
        params
    });
    &PARAMS
}

/// FNV-1a 64-bit hash of a byte slice.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Character trigram shingles of the label (spaces stripped), as 64-bit
/// hashes. Trigrams are what make the similarity tolerant to short edits.
pub fn char_trigram_shingles(label: &str) -> Vec<u64> {
    let compact: String = label.chars().filter(|c| !c.is_whitespace()).collect();
    let chars: Vec<char> = compact.chars().collect();
    if chars.len() < 3 {
        // Pad short labels so they still produce one stable shingle
        let mut padded: String = compact.clone();
        while padded.chars().count() < 3 {
            padded.insert(0, '^');
        }
        return vec![fnv1a(padded.as_bytes())];
    }
    (0..=chars.len() - 3)
        .map(|i| {
            let s: String = chars[i..i + 3].iter().collect();
            fnv1a(s.as_bytes())
        })
        .collect()
}

/// MinHash signature of a shingle set (one value per permutation).
pub fn signature(shingles: &[u64]) -> Vec<u64> {
    let params = perm_params();
    let mut sig = Vec::with_capacity(NUM_PERMS);
    for &(a, b) in params.iter() {
        let mut min = u64::MAX;
        for &h in shingles {
            let v = (a.wrapping_mul(h).wrapping_add(b) % MERSENNE_PRIME) | 1;
            if v < min {
                min = v;
            }
        }
        sig.push(min);
    }
    sig
}

/// Estimated Jaccard similarity between two signatures.
pub fn signature_similarity(a: &[u64], b: &[u64]) -> f64 {
    debug_assert_eq!(a.len(), b.len());
    let equal = a.iter().zip(b.iter()).filter(|(x, y)| x == y).count();
    equal as f64 / a.len() as f64
}

/// Band key for band `band` — two items sharing any band key are candidates.
pub fn band_key(sig: &[u64], band: usize) -> u64 {
    let start = band * ROWS;
    let mut bytes = Vec::with_capacity(ROWS * 8);
    for v in &sig[start..start + ROWS] {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    fnv1a(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_labels_identical_signatures() {
        let a = signature(&char_trigram_shingles("UserService"));
        let b = signature(&char_trigram_shingles("UserService"));
        assert_eq!(a, b);
        assert!((signature_similarity(&a, &b) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn similar_labels_high_similarity() {
        let a = signature(&char_trigram_shingles("graph_query_engine"));
        let b = signature(&char_trigram_shingles("graphqueryengine"));
        assert!(
            signature_similarity(&a, &b) > 0.4,
            "near-identical labels should share most shingles"
        );
    }

    #[test]
    fn different_labels_low_similarity() {
        let a = signature(&char_trigram_shingles("database_connection_pool"));
        let b = signature(&char_trigram_shingles("html_render_visitor"));
        assert!(signature_similarity(&a, &b) < 0.2);
    }

    #[test]
    fn identical_signatures_share_a_band() {
        let a = signature(&char_trigram_shingles("load_graph_db"));
        let b = signature(&char_trigram_shingles("load_graph_db"));
        let shared = (0..BANDS).any(|band| band_key(&a, band) == band_key(&b, band));
        assert!(shared);
    }

    #[test]
    fn short_labels_produce_shingles() {
        assert_eq!(char_trigram_shingles("ab").len(), 1);
        assert_eq!(char_trigram_shingles("abcd").len(), 2);
    }
}
