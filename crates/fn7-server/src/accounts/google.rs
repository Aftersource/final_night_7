//! Google sign-in for a desktop client, without a loopback listener (docs/ACCOUNTS.md "Sign in with Google").
//!
//! The client never talks to Google. It asks us for a `login_url`, opens it in the system browser, and polls us:
//!
//! 1. `POST /api/auth/google/start` → we mint a poll token (for the client), a `state` and a `nonce` (for Google) and
//!    a PKCE verifier (kept here), and answer `{login_url, poll_token, expires}`.
//! 2. Google redirects the browser to `GET <FN7_PUBLIC_URL>/api/auth/google/callback?state=…&code=…`.
//!    [`exchange_code`] trades the code (with the PKCE verifier and the client secret) for an ID token at Google's
//!    token endpoint; [`verify_id_token`] checks it against Google's JWKS: RS256 signature by the key the header
//!    names, issuer, audience (our client id), expiry, and the nonce we minted for this very flow.
//! 3. `POST /api/auth/poll` → `pending`, then once `{session_token, account}`.
//!
//! **Configuration** ([`GoogleConfig::from_env`]): `FN7_GOOGLE_CLIENT_ID`, `FN7_GOOGLE_CLIENT_SECRET` and
//! `FN7_PUBLIC_URL` (the public https origin of this server, no trailing slash). The redirect URI to register in the
//! Google Cloud console is exactly `<FN7_PUBLIC_URL>/api/auth/google/callback`. With any of the three unset, Google
//! sign-in is off and the start route says so. The endpoints default to Google's and can be pointed elsewhere
//! (`FN7_GOOGLE_AUTH_URL`, `FN7_GOOGLE_TOKEN_URL`, `FN7_GOOGLE_JWKS_URL`, `FN7_GOOGLE_ISSUER`): the test suite runs a
//! local provider behind them.
//!
//! Nothing here logs a code, a token, a secret or an email.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use ring::digest;
use ring::signature::{RsaPublicKeyComponents, RSA_PKCS1_2048_8192_SHA256};
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct GoogleConfig {
    pub client_id: String,
    pub client_secret: String,
    /// This server's public origin (`https://play.example.com`).
    pub public_url: String,
    pub auth_url: String,
    pub token_url: String,
    pub jwks_url: String,
    /// Accepted `iss` values.
    pub issuers: Vec<String>,
}

impl GoogleConfig {
    /// `None` unless the client id, the client secret and the public URL are all set.
    pub fn from_env(env: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let get = |k: &str| env(k).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        let issuers = match get("FN7_GOOGLE_ISSUER") {
            Some(i) => vec![i],
            None => vec!["https://accounts.google.com".into(), "accounts.google.com".into()],
        };
        Some(Self {
            client_id: get("FN7_GOOGLE_CLIENT_ID")?,
            client_secret: get("FN7_GOOGLE_CLIENT_SECRET")?,
            public_url: get("FN7_PUBLIC_URL")?.trim_end_matches('/').to_string(),
            auth_url: get("FN7_GOOGLE_AUTH_URL").unwrap_or_else(|| "https://accounts.google.com/o/oauth2/v2/auth".into()),
            token_url: get("FN7_GOOGLE_TOKEN_URL").unwrap_or_else(|| "https://oauth2.googleapis.com/token".into()),
            jwks_url: get("FN7_GOOGLE_JWKS_URL").unwrap_or_else(|| "https://www.googleapis.com/oauth2/v3/certs".into()),
            issuers,
        })
    }

    /// The redirect URI registered with Google.
    pub fn redirect_uri(&self) -> String {
        format!("{}/api/auth/google/callback", self.public_url)
    }

    /// The URL the client opens in the system browser.
    pub fn login_url(&self, state: &str, nonce: &str, code_challenge: &str) -> String {
        let q = [
            ("response_type", "code"),
            ("client_id", self.client_id.as_str()),
            ("redirect_uri", &self.redirect_uri()),
            ("scope", "openid email profile"),
            ("state", state),
            ("nonce", nonce),
            ("code_challenge", code_challenge),
            ("code_challenge_method", "S256"),
            ("prompt", "select_account"),
        ]
        .iter()
        .map(|(k, v)| format!("{k}={}", percent(v)))
        .collect::<Vec<_>>()
        .join("&");
        let sep = if self.auth_url.contains('?') { '&' } else { '?' };
        format!("{}{sep}{q}", self.auth_url)
    }
}

/// Percent-encode everything but the unreserved characters.
pub fn percent(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// base64url without padding.
pub fn b64url(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(B64[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(B64[n as usize & 63] as char);
        }
    }
    out
}

/// Decode base64url (padding optional). `None` on any character outside the alphabet.
pub fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=');
    if s.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        } as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// The PKCE S256 challenge of a verifier.
pub fn pkce_challenge(verifier: &str) -> String {
    b64url(digest::digest(&digest::SHA256, verifier.as_bytes()).as_ref())
}

/// Why a sign-in failed. `code()` is what the client sees; the detail stays in the server log (never a token).
#[derive(Debug, Clone, PartialEq)]
pub enum OidcError {
    /// Google's token endpoint refused the code, or could not be reached.
    Exchange(String),
    /// The ID token is not one we can trust (the reason names the failed check).
    IdToken(&'static str),
    /// The signing keys could not be fetched.
    Keys(String),
}

impl OidcError {
    pub fn code(&self) -> &'static str {
        match self {
            OidcError::Exchange(_) => "token_exchange_failed",
            OidcError::IdToken(_) => "bad_id_token",
            OidcError::Keys(_) => "provider_keys_unavailable",
        }
    }
}

impl std::fmt::Display for OidcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OidcError::Exchange(e) => write!(f, "token exchange: {e}"),
            OidcError::IdToken(why) => write!(f, "id token: {why}"),
            OidcError::Keys(e) => write!(f, "jwks: {e}"),
        }
    }
}

/// One RSA signing key from the provider's JWKS.
#[derive(Debug, Clone, PartialEq)]
pub struct Jwk {
    pub kid: String,
    pub n: Vec<u8>,
    pub e: Vec<u8>,
}

/// The RSA signature keys in a JWKS document.
pub fn parse_jwks(v: &Value) -> Vec<Jwk> {
    v["keys"]
        .as_array()
        .map(|keys| {
            keys.iter()
                .filter(|k| k["kty"] == "RSA" && k["use"].as_str().is_none_or(|u| u == "sig"))
                .filter_map(|k| {
                    Some(Jwk {
                        kid: k["kid"].as_str()?.to_string(),
                        n: b64url_decode(k["n"].as_str()?)?,
                        e: b64url_decode(k["e"].as_str()?)?,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// What we keep of a verified ID token.
#[derive(Debug, Clone, PartialEq)]
pub struct Claims {
    pub sub: String,
    pub email: Option<String>,
    pub email_verified: bool,
    pub name: Option<String>,
    pub given_name: Option<String>,
}

/// Clock skew allowed on `exp` and `iat`, in seconds.
pub const LEEWAY_SECS: i64 = 60;

/// The `kid` an ID token's header names (so the caller can fetch the right key), if it parses at all.
pub fn token_kid(token: &str) -> Option<String> {
    let head = token.split('.').next()?;
    let v: Value = serde_json::from_slice(&b64url_decode(head)?).ok()?;
    v["kid"].as_str().map(str::to_string)
}

/// Verify an ID token: RS256 by one of `keys` (the one its `kid` names), issuer, audience, expiry and nonce.
pub fn verify_id_token(token: &str, keys: &[Jwk], cfg: &GoogleConfig, nonce: &str, now_secs: i64) -> Result<Claims, OidcError> {
    let bad = OidcError::IdToken;
    let mut parts = token.split('.');
    let (Some(h), Some(p), Some(s), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return Err(bad("not a JWT"));
    };
    let header: Value = b64url_decode(h).and_then(|b| serde_json::from_slice(&b).ok()).ok_or(bad("unreadable header"))?;
    // Only RS256: `none`, HS256 (the client secret as an HMAC key) and anything else are refused outright.
    if header["alg"] != "RS256" {
        return Err(bad("algorithm"));
    }
    let kid = header["kid"].as_str().ok_or(bad("no key id"))?;
    let key = keys.iter().find(|k| k.kid == kid).ok_or(bad("unknown key id"))?;
    let sig = b64url_decode(s).ok_or(bad("unreadable signature"))?;
    let signed = &token[..h.len() + 1 + p.len()];
    RsaPublicKeyComponents { n: &key.n, e: &key.e }
        .verify(&RSA_PKCS1_2048_8192_SHA256, signed.as_bytes(), &sig)
        .map_err(|_| bad("signature"))?;
    let c: Value = b64url_decode(p).and_then(|b| serde_json::from_slice(&b).ok()).ok_or(bad("unreadable claims"))?;
    if !c["iss"].as_str().is_some_and(|i| cfg.issuers.iter().any(|x| x == i)) {
        return Err(bad("issuer"));
    }
    let aud_ok = match &c["aud"] {
        Value::String(a) => a == &cfg.client_id,
        // Several audiences: ours must be there and `azp` must be us.
        Value::Array(a) => a.iter().any(|x| x == &Value::String(cfg.client_id.clone())) && c["azp"] == cfg.client_id.as_str(),
        _ => false,
    };
    if !aud_ok {
        return Err(bad("audience"));
    }
    let exp = c["exp"].as_i64().ok_or(bad("no expiry"))?;
    if exp + LEEWAY_SECS <= now_secs {
        return Err(bad("expired"));
    }
    if c["iat"].as_i64().is_some_and(|iat| iat - LEEWAY_SECS > now_secs) {
        return Err(bad("issued in the future"));
    }
    if nonce.is_empty() || c["nonce"].as_str() != Some(nonce) {
        return Err(bad("nonce"));
    }
    let sub = c["sub"].as_str().filter(|s| !s.is_empty() && s.len() <= 255).ok_or(bad("no subject"))?;
    let text = |k: &str| c[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    Ok(Claims {
        sub: sub.to_string(),
        email: text("email"),
        // Google sends a boolean; some providers send "true".
        email_verified: c["email_verified"].as_bool().unwrap_or_else(|| c["email_verified"] == "true"),
        name: text("name"),
        given_name: text("given_name"),
    })
}

/// The provider's signing keys, cached. Refetched when they are an hour old, or when a token names a key we don't
/// have (at most once a minute: a forged `kid` cannot make us hammer the provider).
pub struct KeyCache {
    inner: Mutex<Option<(Instant, Vec<Jwk>)>>,
}

const KEYS_FRESH: Duration = Duration::from_secs(3600);
const KEYS_REFETCH_MIN: Duration = Duration::from_secs(60);

impl Default for KeyCache {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyCache {
    pub fn new() -> Self {
        Self { inner: Mutex::new(None) }
    }

    /// The keys to verify a token signed with `kid`.
    pub async fn keys_for(&self, http: &reqwest::Client, cfg: &GoogleConfig, kid: Option<&str>) -> Result<Vec<Jwk>, OidcError> {
        {
            let g = self.inner.lock().unwrap_or_else(|p| p.into_inner());
            if let Some((at, keys)) = g.as_ref() {
                let has = kid.is_none_or(|k| keys.iter().any(|x| x.kid == k));
                if (has && at.elapsed() < KEYS_FRESH) || at.elapsed() < KEYS_REFETCH_MIN {
                    return Ok(keys.clone());
                }
            }
        }
        let res = http
            .get(&cfg.jwks_url)
            .send()
            .await
            .map_err(|e| OidcError::Keys(e.without_url().to_string()))?;
        if !res.status().is_success() {
            return Err(OidcError::Keys(format!("status {}", res.status().as_u16())));
        }
        let v: Value = res.json().await.map_err(|e| OidcError::Keys(e.without_url().to_string()))?;
        let keys = parse_jwks(&v);
        if keys.is_empty() {
            return Err(OidcError::Keys("no RSA signing keys".into()));
        }
        *self.inner.lock().unwrap_or_else(|p| p.into_inner()) = Some((Instant::now(), keys.clone()));
        Ok(keys)
    }
}

/// Trade an authorization code for an ID token at the provider's token endpoint.
pub async fn exchange_code(http: &reqwest::Client, cfg: &GoogleConfig, code: &str, verifier: &str) -> Result<String, OidcError> {
    let body = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("client_id", cfg.client_id.as_str()),
        ("client_secret", cfg.client_secret.as_str()),
        ("redirect_uri", &cfg.redirect_uri()),
        ("code_verifier", verifier),
    ]
    .iter()
    .map(|(k, v)| format!("{k}={}", percent(v)))
    .collect::<Vec<_>>()
    .join("&");
    let res = http
        .post(&cfg.token_url)
        .header("content-type", "application/x-www-form-urlencoded")
        .header("accept", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|e| OidcError::Exchange(e.without_url().to_string()))?;
    let status = res.status();
    let v: Value = res.json().await.map_err(|_| OidcError::Exchange(format!("status {} with an unreadable body", status.as_u16())))?;
    if !status.is_success() {
        // The provider's error code only ("invalid_grant"): never its description, which can echo the request.
        let code = v["error"].as_str().unwrap_or("error");
        let code: String = code.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(40).collect();
        return Err(OidcError::Exchange(format!("status {} {code}", status.as_u16())));
    }
    v["id_token"].as_str().map(str::to_string).ok_or_else(|| OidcError::Exchange("no id_token in the answer".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64url_round_trips() {
        for data in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar", &[0xfb, 0xff, 0xfe]] {
            let enc = b64url(data);
            assert!(!enc.contains('=') && !enc.contains('+') && !enc.contains('/'));
            assert_eq!(b64url_decode(&enc).unwrap(), data);
        }
        assert_eq!(b64url(b"foobar"), "Zm9vYmFy");
        assert_eq!(b64url(&[0xfb, 0xff, 0xfe]), "-__-");
        assert_eq!(b64url_decode("Zm9vYg=="), Some(b"foob".to_vec()));
        assert_eq!(b64url_decode("Zm9v+g"), None);
        assert_eq!(b64url_decode("Z"), None);
    }

    #[test]
    fn pkce_matches_the_rfc_vector() {
        // RFC 7636 appendix B.
        assert_eq!(pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn config_needs_all_three_and_builds_the_login_url() {
        let env = |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
        assert!(GoogleConfig::from_env(env(&[])).is_none());
        assert!(GoogleConfig::from_env(env(&[("FN7_GOOGLE_CLIENT_ID", "id"), ("FN7_PUBLIC_URL", "https://x")])).is_none());
        assert!(GoogleConfig::from_env(env(&[("FN7_GOOGLE_CLIENT_ID", "id"), ("FN7_GOOGLE_CLIENT_SECRET", " "), ("FN7_PUBLIC_URL", "https://x")])).is_none());
        let cfg = GoogleConfig::from_env(env(&[
            ("FN7_GOOGLE_CLIENT_ID", "id.apps.googleusercontent.com"),
            ("FN7_GOOGLE_CLIENT_SECRET", "shh"),
            ("FN7_PUBLIC_URL", "https://play.example.com/"),
        ]))
        .unwrap();
        assert_eq!(cfg.redirect_uri(), "https://play.example.com/api/auth/google/callback");
        assert_eq!(cfg.issuers.len(), 2);
        let url = cfg.login_url("st 1", "n1", "ch");
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?response_type=code&client_id=id.apps.googleusercontent.com&"));
        assert!(url.contains("redirect_uri=https%3A%2F%2Fplay.example.com%2Fapi%2Fauth%2Fgoogle%2Fcallback"));
        assert!(url.contains("scope=openid%20email%20profile") && url.contains("state=st%201") && url.contains("nonce=n1"));
        assert!(url.contains("code_challenge=ch&code_challenge_method=S256"));
        assert!(!url.contains("shh"), "the client secret never goes to the browser");
    }
}
