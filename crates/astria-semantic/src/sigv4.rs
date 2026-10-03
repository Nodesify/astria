//! AWS Signature Version 4 request signing for the Bedrock backend.
//!
//! Self-contained on purpose: HMAC-SHA256 is ~15 lines on top of the sha2
//! crate the workspace already depends on, and the signing inputs here are
//! exactly one service (bedrock), one method (POST), and an empty query
//! string — a generic SigV4 library would be dead weight.
//!
//! Canonical request (SigV4 spec):
//! ```text
//! method \n canonical-uri \n canonical-query \n
//! canonical-headers \n signed-headers \n payload-hash
//! ```

use sha2::{Digest, Sha256};

pub(crate) fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex(&hasher.finalize())
}

/// HMAC-SHA256: H((K⊕opad) ∥ H((K⊕ipad) ∥ message)) per RFC 2104.
pub(crate) fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        let mut hasher = Sha256::new();
        hasher.update(key);
        k[..32].copy_from_slice(&hasher.finalize());
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; BLOCK];
    let mut opad = [0x5cu8; BLOCK];
    for i in 0..BLOCK {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(data);
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner);
    outer.finalize().into()
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// `YYYYMMDD'T'HHMMSS'Z'` and `YYYYMMDD` from unix seconds — the two date
/// forms SigV4 needs. Hand-rolled because the only inputs are an epoch and
/// the civil-date algorithm, and no workspace crate carries a date library.
pub(crate) fn amz_dates(unix_secs: u64) -> (String, String) {
    let days = (unix_secs / 86_400) as i64;
    let secs_of_day = unix_secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    let (h, mi, s) = (
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60,
    );
    (
        format!("{y:04}{m:02}{d:02}T{h:02}{mi:02}{s:02}Z"),
        format!("{y:04}{m:02}{d:02}"),
    )
}

/// Days-since-epoch → (year, month, day). Howard Hinnant's `civil_from_days`;
/// valid for the whole `u64` unix-seconds range astria will ever see.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Everything needed to sign one Bedrock request. `path` must already be
/// URI-encoded — the canonical request signs the encoded form, so the same
/// string must go into the URL.
pub(crate) struct SigV4Request<'a> {
    pub method: &'a str,
    pub host: &'a str,
    pub path: &'a str,
    pub payload: &'a [u8],
    pub access_key: &'a str,
    pub secret_key: &'a str,
    pub session_token: Option<&'a str>,
    pub region: &'a str,
    pub service: &'a str,
    pub amz_date: &'a str,
    pub short_date: &'a str,
}

impl SigV4Request<'_> {
    /// All headers to send with the request, in send order: host, the
    /// x-amz-* signing headers, then authorization. Nothing here is secret
    /// except the signature itself (credentials never leave the header).
    pub fn sign(&self) -> Vec<(String, String)> {
        let payload_hash = sha256_hex(self.payload);
        let mut canonical_headers = format!(
            "host:{}\nx-amz-content-sha256:{}\nx-amz-date:{}\n",
            self.host, payload_hash, self.amz_date
        );
        let mut signed_headers =
            String::from("host;x-amz-content-sha256;x-amz-date");
        if let Some(token) = self.session_token {
            canonical_headers.push_str(&format!("x-amz-security-token:{token}\n"));
            signed_headers.push_str(";x-amz-security-token");
        }

        let canonical_request = format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            self.method,
            self.path,
            "", // canonical query: Bedrock Converse takes none
            canonical_headers,
            signed_headers,
            payload_hash
        );
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{}\n{}/{}/{}/aws4_request\n{}",
            self.amz_date,
            self.short_date,
            self.region,
            self.service,
            sha256_hex(canonical_request.as_bytes())
        );
        let key = hmac_sha256(
            format!("AWS4{}", self.secret_key).as_bytes(),
            self.short_date.as_bytes(),
        );
        let key = hmac_sha256(&key, self.region.as_bytes());
        let key = hmac_sha256(&key, self.service.as_bytes());
        let key = hmac_sha256(&key, b"aws4_request");
        let signature = hex(&hmac_sha256(&key, string_to_sign.as_bytes()));

        let mut headers = vec![
            ("host".to_string(), self.host.to_string()),
            ("x-amz-content-sha256".to_string(), payload_hash.clone()),
            ("x-amz-date".to_string(), self.amz_date.to_string()),
        ];
        if let Some(token) = self.session_token {
            headers.push(("x-amz-security-token".to_string(), token.to_string()));
        }
        headers.push((
            "authorization".to_string(),
            format!(
                "AWS4-HMAC-SHA256 Credential={}/{}/{}/{}/aws4_request, SignedHeaders={}, Signature={}",
                self.access_key, self.short_date, self.region, self.service,
                signed_headers, signature
            ),
        ));
        headers
    }
}

/// Percent-encode a Bedrock model id for the URL path. Model ids contain
/// `:` and `/` (inference profiles) that must be encoded; `.` `-` `_` and
/// alphanumerics are safe per RFC 3986 unreserved set.
pub(crate) fn encode_model_id(model: &str) -> String {
    let mut out = String::with_capacity(model.len());
    for b in model.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_sha256_rfc4231_case1() {
        let key = [0x0bu8; 20];
        let mac = hmac_sha256(&key, b"Hi There");
        assert_eq!(
            hex(&mac),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn hmac_sha256_long_key_falls_back_to_hash() {
        // RFC 4231 case 6: a key longer than the block size is hashed first.
        let key = [0xaau8; 131];
        let mac = hmac_sha256(&key, b"Test Using Larger Than Block-Size Key - Hash Key First");
        assert_eq!(
            hex(&mac),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    #[test]
    fn sha256_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn amz_dates_format_epoch_and_known_instant() {
        assert_eq!(amz_dates(0), ("19700101T000000Z".to_string(), "19700101".to_string()));
        assert_eq!(
            amz_dates(1_700_000_000),
            ("20231114T221320Z".to_string(), "20231114".to_string())
        );
        // Leap-day boundary: 2024-02-29T00:00:00Z = 1709164800
        assert_eq!(
            amz_dates(1_709_164_800),
            ("20240229T000000Z".to_string(), "20240229".to_string())
        );
    }

    #[test]
    fn signs_a_bedrock_request_deterministically() {
        // Reference computed independently (Python hashlib/hmac) for exactly
        // these inputs — pins the whole canonical-request pipeline, not just
        // the primitives.
        let req = SigV4Request {
            method: "POST",
            host: "bedrock-runtime.us-east-1.amazonaws.com",
            path: "/model/anthropic.claude-3-5-sonnet-20241022-v2%3A0/converse",
            payload: b"{\"messages\":[]}",
            access_key: "AKIDEXAMPLE",
            secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            session_token: None,
            region: "us-east-1",
            service: "bedrock",
            amz_date: "20231114T221320Z",
            short_date: "20231114",
        };
        let headers = req.sign();
        let find = |name: &str| {
            headers
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        assert_eq!(
            find("x-amz-content-sha256"),
            "5e4ce7b36ba37b78a5d5f9fd08e6b7b54ba6879d651aa46ec9e1d6fa24ebe30a"
        );
        assert_eq!(
            find("authorization"),
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20231114/us-east-1/bedrock/aws4_request, \
             SignedHeaders=host;x-amz-content-sha256;x-amz-date, \
             Signature=56380140b9de434bf40654e0c4e501a69502cc10dd27bd800602c1fdb95d5eff"
        );
        assert!(!headers.iter().any(|(k, _)| k == "x-amz-security-token"));
    }

    #[test]
    fn session_token_is_signed_and_sent() {
        let req = SigV4Request {
            method: "POST",
            host: "h",
            path: "/p",
            payload: b"{}",
            access_key: "AK",
            secret_key: "SK",
            session_token: Some("TOKEN"),
            region: "us-east-1",
            service: "bedrock",
            amz_date: "20231114T221320Z",
            short_date: "20231114",
        };
        let headers = req.sign();
        assert!(headers.iter().any(|(k, v)| k == "x-amz-security-token" && v == "TOKEN"));
        let auth = headers.iter().find(|(k, _)| k == "authorization").unwrap();
        assert!(auth.1.contains("SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token"));
    }

    #[test]
    fn model_id_encoding() {
        assert_eq!(
            encode_model_id("anthropic.claude-3-5-sonnet-20241022-v2:0"),
            "anthropic.claude-3-5-sonnet-20241022-v2%3A0"
        );
        assert_eq!(
            encode_model_id("us.amazon.nova-pro-v1:0"),
            "us.amazon.nova-pro-v1%3A0"
        );
        assert_eq!(encode_model_id("plain-model"), "plain-model");
    }
}
