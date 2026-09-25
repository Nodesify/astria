// Stable node-id normalization.
// Ported from upstream astria v8 ids.py: lowercase-folding and NFKC do not
// commute (e.g. Greek ypogegrammeni combinations), so we iterate both to a
// fixpoint. The result is idempotent and case-stable by construction, which
// eliminates a whole class of duplicate-id bugs upstream spent its 0.9.x
// series fixing.

use unicode_normalization::UnicodeNormalization;

/// Maximum fixpoint iterations. Convergence takes at most a couple of rounds
/// in practice; the cap guards against pathological input.
const MAX_ROUNDS: usize = 6;

/// Normalize a raw string into a stable id segment:
/// lowercase + NFKC to a fixpoint, Unicode word characters kept, everything
/// else collapsed to single underscores, no leading/trailing underscores.
pub fn normalize_id(raw: &str) -> String {
    let mut current: String = raw.to_string();
    for _ in 0..MAX_ROUNDS {
        let folded: String = current.chars().flat_map(|c| c.to_lowercase()).collect();
        let next: String = folded.nfkc().collect();
        if next == current {
            break;
        }
        current = next;
    }

    let mut out = String::with_capacity(current.len());
    let mut pending_underscore = false;
    for ch in current.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            if ch == '_' {
                pending_underscore = !out.is_empty();
            } else {
                if pending_underscore {
                    out.push('_');
                    pending_underscore = false;
                }
                out.push(ch);
            }
        } else if !out.is_empty() {
            pending_underscore = true;
        }
    }
    out
}

/// Build a stable id from hierarchical parts, joined with `::`.
pub fn make_id(parts: &[&str]) -> String {
    parts
        .iter()
        .filter(|p| !p.trim().is_empty())
        .map(|p| normalize_id(p))
        .collect::<Vec<_>>()
        .join("::")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_normalization() {
        assert_eq!(normalize_id("Greeter"), "greeter");
        assert_eq!(normalize_id("My-Class!"), "my_class");
        assert_eq!(normalize_id("  spaced   out  "), "spaced_out");
        assert_eq!(
            normalize_id("___leading_and_trailing___"),
            "leading_and_trailing"
        );
        assert_eq!(normalize_id("()"), "");
    }

    #[test]
    fn idempotent() {
        // normalize(normalize(x)) == normalize(x) for tricky inputs
        for input in ["İstanbul", "Straße", "Ⅻ", "ﬁle", "ΑΘΗΝΑ", "a::b__c"] {
            let once = normalize_id(input);
            let twice = normalize_id(&once);
            assert_eq!(once, twice, "not idempotent for {input:?}");
        }
    }

    #[test]
    fn caseless_stable() {
        // Differently-cased spellings of the same name produce the same id
        assert_eq!(normalize_id("APIHandler"), normalize_id("apihandler"));
        assert_eq!(normalize_id("XMLParser"), normalize_id("xmlparser"));
    }

    #[test]
    fn unicode_compat_forms_merge() {
        // NFKC folds compatibility characters: ﬁ (U+FB01) -> fi
        assert_eq!(normalize_id("ﬁle"), normalize_id("file"));
        // Ⅻ (Roman numeral twelve) -> xii
        assert_eq!(normalize_id("Ⅻ"), "xii");
    }

    #[test]
    fn cjk_and_cyrillic_survive() {
        assert_eq!(normalize_id("図書館"), "図書館");
        assert_eq!(normalize_id("Библиотека"), "библиотека");
    }

    #[test]
    fn make_id_joins_parts() {
        assert_eq!(
            make_id(&["src_lib", "Greeter", "greet()"]),
            "src_lib::greeter::greet"
        );
        assert_eq!(make_id(&["a", "", "b"]), "a::b");
    }
}
