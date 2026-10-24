//! HTTP/1.1 GET and POST for `http` and `https`.
//!
//! Windows HTTPS uses Schannel with certificate verification on. Linux HTTPS
//! uses OpenSSL `libssl` and `libcrypto` with peer verification and a hostname
//! check. Other targets refuse `https`. Each exchange opens one TCP socket and
//! sends `Connection: close`. HTTP/2 is absent. A connection pool is absent.
//!
//! `flux-browser` page loads call [`fetch`] and [`fetch_post`]. Those functions
//! block a tracker hostname before connect, follow at most [`MAX_REDIRECTS`]
//! redirects, and refuse an `https` to `http` downgrade. Script `fetch()` is
//! same-origin only. The Rust interpreter calls a host in `flux-browser`, and
//! that host calls [`http_get_with_cookie`] with the session cookie jar. The
//! numeric VM does not call this crate. `gzip` and `deflate` bodies go through
//! `flux-compress`. Cookie and HSTS snapshots go through `flux-json` in [`policy`].
//!
//! Limits: [`MAX_RESPONSE`] raw bytes, [`MAX_DECOMPRESSED_BODY`] after inflate,
//! [`MAX_HEADER_BYTES`], [`MAX_HEADER_COUNT`], and [`MAX_REDIRECTS`] hops.

pub mod policy;
pub mod shield;
#[cfg(any(windows, target_os = "linux"))]
mod tls;

pub use policy::{
    ContentSecurityPolicy, Cookie, CookieJar, CspDirective, CspSource, HstsEntry, HstsStore,
    MAX_COOKIE_BYTES, MAX_COOKIES_PER_HOST, MAX_COOKIES_TOTAL, MAX_HSTS_HOSTS, SameSite,
    allows_corp_subresource, is_same_site, sha256_digest, sri_sha256_token,
    verify_subresource_integrity,
};
pub use shield::{NetworkShield, ShieldBlockReason, is_url_blocked_by_default_shield};

use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// Maximum raw response bytes, including headers, before decompression (8 MiB).
///
/// The cleartext and TLS reads stop with [`NetError::BodyTooLarge`] at this size.
/// Chunked bodies use the same cap on the decoded chunks.
pub const MAX_RESPONSE: usize = 8 * 1024 * 1024;

/// Maximum bytes after `gzip` or `deflate` decompression (16 MiB).
///
/// A larger inflate from `flux-compress` becomes [`NetError::BodyTooLarge`].
pub const MAX_DECOMPRESSED_BODY: usize = 16 * 1024 * 1024;

/// Maximum header-block bytes before the blank line (64 KiB).
///
/// A larger block is [`NetError::InvalidResponse`], not [`NetError::BodyTooLarge`].
pub const MAX_HEADER_BYTES: usize = 64 * 1024;

/// Maximum header fields accepted.
///
/// The blank line is not counted. A further field is [`NetError::InvalidResponse`].
/// A line with no colon is skipped and does not count.
pub const MAX_HEADER_COUNT: usize = 128;

/// Maximum redirects followed by [`fetch`] and [`fetch_post`].
///
/// The loop allows the first response plus this many hops. One more redirect
/// status after that returns [`NetError::TooManyRedirects`].
pub const MAX_REDIRECTS: usize = 5;

/// An `http` or `https` URL with no fragment and no userinfo.
///
/// [`parse_url`] is the only constructor. IPv6 brackets are rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Url {
    /// Lowercased `http` or `https`.
    pub scheme: String,
    /// Lowercased host. No brackets and no userinfo.
    pub host: String,
    /// Explicit port, or 80 for `http` and 443 for `https`. Port 0 is rejected.
    pub port: u16,
    /// Path beginning with `/`. An empty path is stored as `/`.
    pub path: String,
    /// Query without `?`, if one was present. Not decoded.
    pub query: Option<String>,
}

impl Url {
    /// Default port for `http` and `https`. Any other scheme is `None`.
    pub fn default_port(scheme: &str) -> Option<u16> {
        match scheme {
            "http" => Some(80),
            "https" => Some(443),
            _ => None,
        }
    }

    /// `scheme://host`, or `scheme://host:port` when the port is not the default.
    pub fn authority(&self) -> String {
        if Self::default_port(&self.scheme) == Some(self.port) {
            format!("{}://{}", self.scheme, self.host)
        } else {
            format!("{}://{}:{}", self.scheme, self.host, self.port)
        }
    }
}

/// One parsed HTTP/1.1 response.
///
/// The body is after chunk decoding and after `gzip` or `deflate` decompression.
/// Header names are lowercased. The reason phrase may be empty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    /// Status code from the status line. Not range-checked.
    pub status: u16,
    /// Reason phrase with internal whitespace collapsed to single spaces. May be empty.
    pub reason: String,
    /// Header names lowercased. Repeated names are kept in order.
    pub headers: Vec<(String, String)>,
    /// Decoded body. Not truncated to a character boundary.
    pub body: Vec<u8>,
}

impl Response {
    /// First header value for `name`, compared case-insensitively. Names are already lowercased.
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.as_str())
    }

    /// Every value for `name`, in arrival order. Used for repeated `Set-Cookie` fields.
    pub fn headers_named(&self, name: &str) -> Vec<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .filter(|(key, _)| *key == name)
            .map(|(_, value)| value.as_str())
            .collect()
    }
}

/// Failure from parsing, policy, or the socket.
#[derive(Debug)]
pub enum NetError {
    /// The scheme is not `http` or `https`, including `https` on a target with no TLS backend.
    UnsupportedScheme,
    /// The URL is empty, has userinfo, has controls, or failed host or port checks.
    InvalidUrl,
    /// The status line, headers, chunks, or content encoding could not be read.
    InvalidResponse,
    /// More than [`MAX_REDIRECTS`] redirect statuses were followed.
    TooManyRedirects,
    /// A redirect moved from `https` to `http`.
    InsecureRedirectDowngrade,
    /// The shield refused the host or path before connect. The string is the display reason.
    BlockedByShield(String),
    /// The raw response or the decompressed body exceeded its cap.
    BodyTooLarge,
    /// DNS, TCP, timeout, or TLS failure, including a failed certificate check.
    Io(std::io::Error),
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedScheme => write!(f, "only http and https URLs can be fetched"),
            Self::InvalidUrl => write!(f, "the URL could not be read"),
            Self::InvalidResponse => write!(f, "the response could not be read"),
            Self::TooManyRedirects => write!(f, "the page redirected too many times"),
            Self::InsecureRedirectDowngrade => {
                write!(f, "blocked insecure redirect from https to http")
            }
            Self::BlockedByShield(reason) => write!(f, "blocked by Flux Shield: {reason}"),
            Self::BodyTooLarge => write!(f, "the response exceeds safety size limits"),
            Self::Io(err) => write!(f, "the connection failed: {err}"),
        }
    }
}

impl std::error::Error for NetError {}

/// Parse an absolute `http` or `https` URL.
///
/// Trims whitespace, rejects ASCII controls, space, and DEL, and drops the fragment.
/// The scheme must be `http` or `https`. An empty authority, a leading `[` (IPv6),
/// or `@` (userinfo) is [`NetError::InvalidUrl`]. The host is lowercased, at most
/// 253 characters, and limited to ASCII letters, digits, `.`, and `-`. It must
/// not start with `.` or `-`. The port must be a non-zero `u16`. A missing path
/// becomes `/`.
pub fn parse_url(input: &str) -> Result<Url, NetError> {
    let input = input.trim();
    if input.bytes().any(|b| b < 0x20 || b == 0x7F || b == b' ') {
        return Err(NetError::InvalidUrl);
    }
    let (scheme, rest) = input.split_once("://").ok_or(NetError::InvalidUrl)?;
    let scheme = scheme.to_ascii_lowercase();
    let default_port = Url::default_port(&scheme).ok_or(NetError::UnsupportedScheme)?;
    if rest.is_empty() {
        return Err(NetError::InvalidUrl);
    }
    let rest = rest.split('#').next().unwrap_or("");
    let (authority, path_and_query) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    // Reject an empty authority, IPv6 brackets, and userinfo. Userinfo is not parsed.
    if authority.is_empty() || authority.starts_with('[') || authority.contains('@') {
        return Err(NetError::InvalidUrl);
    }
    let (host, port) = if let Some((host, port)) = authority.rsplit_once(':') {
        if host.is_empty() {
            return Err(NetError::InvalidUrl);
        }
        let port: u16 = port.parse().map_err(|_| NetError::InvalidUrl)?;
        if port == 0 {
            return Err(NetError::InvalidUrl);
        }
        (host.to_ascii_lowercase(), port)
    } else {
        (authority.to_ascii_lowercase(), default_port)
    };
    if !is_valid_hostname(&host) {
        return Err(NetError::InvalidUrl);
    }
    let (path, query) = match path_and_query.split_once('?') {
        Some((path, query)) => (empty_as_root(path), Some(query.to_string())),
        None => (empty_as_root(path_and_query), None),
    };
    Ok(Url {
        scheme,
        host,
        port,
        path,
        query,
    })
}

/// Hostname shape used by [`parse_url`].
///
/// Empty, longer than 253, a leading `.` or `-`, or any character other than
/// ASCII alphanumeric, `.`, or `-` fails. This accepts dotted IPv4 text and
/// rejects bracketed IPv6 before this function is called.
fn is_valid_hostname(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && !host.starts_with('.')
        && !host.starts_with('-')
        && host
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-')
}

/// Whether `base` and `target` share scheme, host, and port.
///
/// `target` is resolved against `base` first. An empty target, a failed resolve,
/// or a failed parse is not same-origin. For an `http` or `https` base, scheme,
/// host, and port must all match. For a `flux://` base, the resolved URL must
/// also start with `flux://`. Any other base is same-origin only when the
/// resolved reference has no scheme.
pub fn is_same_origin(base: &str, target: &str) -> bool {
    let base = base.trim();
    let target = target.trim();
    if target.is_empty() {
        return false;
    }
    let Some(resolved) = resolve_url(base, target) else {
        return false;
    };
    if base.starts_with("http://") || base.starts_with("https://") {
        let (Ok(a), Ok(b)) = (parse_url(base), parse_url(&resolved)) else {
            return false;
        };
        a.scheme == b.scheme && a.host.eq_ignore_ascii_case(&b.host) && a.port == b.port
    } else if base.starts_with("flux://") {
        resolved.starts_with("flux://")
    } else {
        !resolved.contains("://") && !has_scheme(&resolved)
    }
}

/// Store an empty path as `/`. Other paths are kept, including those without a leading slash.
fn empty_as_root(path: &str) -> String {
    if path.is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    }
}

/// Resolve `reference` against `base`.
///
/// Returns `None` for an empty reference, a fragment-only reference, ASCII
/// controls, or DEL. The fragment is stripped before resolution. A `//` reference
/// keeps the base scheme and must parse as `http` or `https`. A `flux://`
/// reference is kept only when `base` is also `flux://`. Any other scheme that
/// contains `://` must parse as `http` or `https`, so `file:` and similar
/// schemes fail. A scheme without `://`, such as `javascript:`, also fails.
/// Relative references on `http` or `https` are path-joined and `.` and `..`
/// are collapsed. Other bases use file-path joining, which rejects absolute
/// paths and a `..` that leaves the base directory.
pub fn resolve_url(base: &str, reference: &str) -> Option<String> {
    let reference = reference.trim();
    if reference.is_empty()
        || reference.starts_with('#')
        || reference.bytes().any(|b| b < 0x20 || b == 0x7F)
    {
        return None;
    }
    let reference = reference.split('#').next().unwrap_or("").trim();
    if reference.is_empty() {
        return None;
    }
    if let Some(rest) = reference.strip_prefix("//") {
        let base_url = parse_url(base).ok()?;
        let candidate = format!("{}://{rest}", base_url.scheme);
        parse_url(&candidate).ok()?;
        return Some(candidate);
    }
    if reference.starts_with("flux://") {
        // A flux:// target is accepted only from a flux:// base.
        if base.starts_with("flux://") {
            return Some(reference.to_string());
        }
        return None;
    }
    if reference.contains("://") {
        parse_url(reference).ok()?;
        return Some(reference.to_string());
    }
    if has_scheme(reference) {
        return None;
    }
    if base.starts_with("flux://") {
        return Some(format!("flux://{}", reference.trim_start_matches('/')));
    }
    if base.starts_with("http://") || base.starts_with("https://") {
        let base_url = parse_url(base).ok()?;
        let authority = base_url.authority();
        let (ref_path, query) = match reference.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (reference, None),
        };
        let combined = if ref_path.starts_with('/') {
            ref_path.to_string()
        } else {
            let dir = match base_url.path.rfind('/') {
                Some(index) => &base_url.path[..=index],
                None => "/",
            };
            format!("{dir}{ref_path}")
        };
        let normalized = normalize_http_path(&combined);
        return Some(match query {
            Some(query) => format!("{authority}{normalized}?{query}"),
            None => format!("{authority}{normalized}"),
        });
    }
    resolve_file_path(base, reference)
}

/// Whether `reference` begins with a scheme token and a colon.
///
/// The token must be non-empty and contain only ASCII alphanumeric, `+`, `-`,
/// or `.`. A slash or backslash in the token means this is not a scheme, so a
/// Windows path is not treated as one. Used to reject `javascript:` and similar
/// references that are not `http`, `https`, or `flux`.
fn has_scheme(reference: &str) -> bool {
    let Some((prefix, _)) = reference.split_once(':') else {
        return false;
    };
    !prefix.is_empty()
        && !prefix.contains('/')
        && !prefix.contains('\\')
        && prefix
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
}

/// Collapse `.` and `..` in an HTTP path.
///
/// Empty segments and `.` are dropped. `..` pops one segment and does not fail
/// when the stack is already empty, so `..` above the root stays at `/`.
/// A trailing slash is preserved except for the root.
fn normalize_http_path(path: &str) -> String {
    let mut stack = Vec::new();
    let trailing_slash = path.ends_with('/') && path.len() > 1;
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            segment => stack.push(segment),
        }
    }
    if stack.is_empty() {
        "/".to_string()
    } else if trailing_slash {
        format!("/{}/", stack.join("/"))
    } else {
        format!("/{}", stack.join("/"))
    }
}

/// Join a relative file reference onto the directory of `base`.
///
/// Returns `None` when the reference is empty, absolute (`/` or `\`), contains
/// `:`, or a `..` would pop past the base directory. The query is discarded.
/// `.` segments are ignored. Backslashes in the base and the reference are
/// treated as separators.
fn resolve_file_path(base: &str, reference: &str) -> Option<String> {
    let ref_path = reference.split('?').next().unwrap_or("").trim();
    if ref_path.is_empty()
        || ref_path.starts_with('/')
        || ref_path.starts_with('\\')
        || ref_path.contains(':')
    {
        return None;
    }
    let base_norm = base.replace('\\', "/");
    let mut stack: Vec<&str> = match base_norm.rfind('/') {
        Some(index) => base_norm[..index]
            .split('/')
            .filter(|part| !part.is_empty() && *part != ".")
            .collect(),
        None => Vec::new(),
    };
    for part in ref_path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                stack.pop()?;
            }
            segment => stack.push(segment),
        }
    }
    if stack.is_empty() {
        None
    } else {
        Some(stack.join("/"))
    }
}

/// Parse one HTTP/1.1 response from bytes already read off the socket.
///
/// Requires a `\r\n\r\n` header terminator. A header block larger than
/// [`MAX_HEADER_BYTES`] is [`NetError::InvalidResponse`]. The status line must
/// have a version token and a `u16` status. The version is not checked. The
/// remaining tokens are the reason phrase. Header names are lowercased. A line
/// without `:` is skipped. More than [`MAX_HEADER_COUNT`] fields fails.
/// `Transfer-Encoding: chunked` wins over `Content-Length`. A declared length
/// above [`MAX_RESPONSE`], or a body above that cap, is [`NetError::BodyTooLarge`].
/// A shorter body than `Content-Length` is kept as read. Bytes past
/// `Content-Length` are dropped. The body is then decoded from `gzip` or `deflate`.
pub fn parse_http_response(bytes: &[u8]) -> Result<Response, NetError> {
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or(NetError::InvalidResponse)?;
    // Header block cap. Oversized headers are an invalid response, not a body-size error.
    if split > MAX_HEADER_BYTES {
        return Err(NetError::InvalidResponse);
    }
    let header_text =
        std::str::from_utf8(&bytes[..split]).map_err(|_| NetError::InvalidResponse)?;
    let raw_body = &bytes[split + 4..];
    let mut lines = header_text.lines();
    let status_line = lines.next().ok_or(NetError::InvalidResponse)?;
    let mut parts = status_line.split_whitespace();
    let _version = parts.next().ok_or(NetError::InvalidResponse)?;
    let status = parts
        .next()
        .ok_or(NetError::InvalidResponse)?
        .parse()
        .map_err(|_| NetError::InvalidResponse)?;
    let reason = parts.collect::<Vec<_>>().join(" ");
    let mut headers = Vec::new();
    for line in lines {
        // Header count cap. Lines without a colon are ignored and do not count.
        if headers.len() >= MAX_HEADER_COUNT {
            return Err(NetError::InvalidResponse);
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
    }
    let raw_decoded = if is_chunked(&headers) {
        decode_chunked(raw_body)?
    } else {
        let mut body = raw_body.to_vec();
        if let Some(length) = content_length(&headers) {
            // A declared length above the raw cap fails before the body is kept.
            if length > MAX_RESPONSE {
                return Err(NetError::BodyTooLarge);
            }
            if body.len() > length {
                body.truncate(length);
            }
        }
        if body.len() > MAX_RESPONSE {
            return Err(NetError::BodyTooLarge);
        }
        body
    };
    let body = decode_content_encoding(&headers, raw_decoded)?;
    Ok(Response {
        status,
        reason,
        headers,
        body,
    })
}

/// Decode `Content-Encoding` after chunk or length handling.
///
/// Missing, empty, and `identity` return the body unchanged. `gzip` and
/// `x-gzip` use bounded gzip inflate. `deflate` tries zlib first, then raw
/// deflate. An output over [`MAX_DECOMPRESSED_BODY`] is [`NetError::BodyTooLarge`].
/// Any other inflate error is [`NetError::InvalidResponse`]. An unrecognized
/// encoding is returned unchanged, still compressed.
fn decode_content_encoding(
    headers: &[(String, String)],
    body: Vec<u8>,
) -> Result<Vec<u8>, NetError> {
    let Some(enc) = headers
        .iter()
        .find(|(name, _)| name == "content-encoding")
        .map(|(_, val)| val.trim().to_ascii_lowercase())
    else {
        return Ok(body);
    };
    match enc.as_str() {
        "" | "identity" => Ok(body),
        "gzip" | "x-gzip" => flux_compress::decompress_gzip_bounded(&body, MAX_DECOMPRESSED_BODY)
            .map_err(|err| match err {
                flux_compress::CompressError::OutputTooLarge { .. } => NetError::BodyTooLarge,
                _ => NetError::InvalidResponse,
            }),
        "deflate" => flux_compress::decompress_zlib_bounded(&body, MAX_DECOMPRESSED_BODY)
            .or_else(|_| flux_compress::decompress_deflate_bounded(&body, MAX_DECOMPRESSED_BODY))
            .map_err(|err| match err {
                flux_compress::CompressError::OutputTooLarge { .. } => NetError::BodyTooLarge,
                _ => NetError::InvalidResponse,
            }),
        // Unknown encodings are not rejected. The caller receives the raw bytes.
        _ => Ok(body),
    }
}

/// First `Content-Length` that parses as `usize`. A missing or non-numeric value is ignored.
fn content_length(headers: &[(String, String)]) -> Option<usize> {
    headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse().ok())
}

/// Whether any `Transfer-Encoding` list contains `chunked`, case-insensitively.
///
/// Comma-separated tokens are trimmed. Other transfer encodings are not decoded.
fn is_chunked(headers: &[(String, String)]) -> bool {
    headers.iter().any(|(name, value)| {
        name == "transfer-encoding"
            && value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("chunked"))
    })
}

/// Decode a chunked body.
///
/// Each size line is hex before an optional `;` extension. The extension is
/// ignored. A missing CRLF, a non-hex size, a short chunk, or a chunk that is
/// not followed by CRLF is [`NetError::InvalidResponse`]. A zero size ends the
/// body. Trailers after that chunk are not parsed. The summed chunk bytes must
/// stay within [`MAX_RESPONSE`] or the result is [`NetError::BodyTooLarge`].
fn decode_chunked(mut input: &[u8]) -> Result<Vec<u8>, NetError> {
    let mut out = Vec::new();
    loop {
        let line_end = input
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or(NetError::InvalidResponse)?;
        let size_line =
            std::str::from_utf8(&input[..line_end]).map_err(|_| NetError::InvalidResponse)?;
        let size_hex = size_line.split(';').next().unwrap_or("").trim();
        let chunk_len =
            usize::from_str_radix(size_hex, 16).map_err(|_| NetError::InvalidResponse)?;
        input = &input[line_end + 2..];
        if chunk_len == 0 {
            break;
        }
        if input.len() < chunk_len + 2 {
            return Err(NetError::InvalidResponse);
        }
        // Decoded chunk cap. The raw read is also capped before this runs.
        if out.len() + chunk_len > MAX_RESPONSE {
            return Err(NetError::BodyTooLarge);
        }
        out.extend_from_slice(&input[..chunk_len]);
        if &input[chunk_len..chunk_len + 2] != b"\r\n" {
            return Err(NetError::InvalidResponse);
        }
        input = &input[chunk_len + 2..];
    }
    Ok(out)
}

/// HTTP/1.1 GET with no `Cookie` header. Does not follow redirects and does not apply the shield.
pub fn http_get(url: &Url) -> Result<Response, NetError> {
    http_exchange(url, build_request_with_cookie(url, None).as_bytes())
}

/// HTTP/1.1 GET. `cookie_header` is sent only when it is `Some` and non-empty.
/// Does not follow redirects and does not apply the shield.
pub fn http_get_with_cookie(url: &Url, cookie_header: Option<&str>) -> Result<Response, NetError> {
    http_exchange(
        url,
        build_request_with_cookie(url, cookie_header).as_bytes(),
    )
}

/// HTTP/1.1 POST with no `Cookie` header. Does not follow redirects and does not apply the shield.
pub fn http_post(url: &Url, content_type: &str, body: &[u8]) -> Result<Response, NetError> {
    let request = build_post_request_with_cookie(url, content_type, body, None);
    http_exchange(url, &request)
}

/// HTTP/1.1 POST. `cookie_header` is sent only when it is `Some` and non-empty.
/// Does not follow redirects and does not apply the shield.
pub fn http_post_with_cookie(
    url: &Url,
    content_type: &str,
    body: &[u8],
    cookie_header: Option<&str>,
) -> Result<Response, NetError> {
    let request = build_post_request_with_cookie(url, content_type, body, cookie_header);
    http_exchange(url, &request)
}

/// Open one TCP connection, exchange one HTTP/1.1 message, and close it.
///
/// Read and write timeouts are 15 seconds. `https` uses [`tls::https_exchange`]
/// on Windows and Linux. On any other target `https` is [`NetError::UnsupportedScheme`].
/// The raw bytes, before header parsing, are capped at [`MAX_RESPONSE`].
/// A timeout or `WouldBlock` with an empty buffer is an I/O error. The same
/// condition after some bytes stops the read and parses what arrived.
/// There is no connection pool and no second request on the socket.
fn http_exchange(url: &Url, request: &[u8]) -> Result<Response, NetError> {
    if url.scheme != "http" && url.scheme != "https" {
        return Err(NetError::UnsupportedScheme);
    }
    let mut stream =
        TcpStream::connect(format!("{}:{}", url.host, url.port)).map_err(NetError::Io)?;
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .map_err(NetError::Io)?;
    stream
        .set_write_timeout(Some(Duration::from_secs(15)))
        .map_err(NetError::Io)?;

    if url.scheme == "https" {
        // TLS only on Windows (Schannel) and Linux (OpenSSL). Verification is on in tls.rs.
        #[cfg(any(windows, target_os = "linux"))]
        {
            let plain = tls::https_exchange(stream, &url.host, request, MAX_RESPONSE)?;
            return parse_http_response(&plain);
        }
        // No TLS backend on this target. Do not send the request in cleartext.
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            return Err(NetError::UnsupportedScheme);
        }
    }

    stream.write_all(request).map_err(NetError::Io)?;

    let mut buf = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(size) => {
                // Raw response cap, headers included, before decompression.
                if buf.len() + size > MAX_RESPONSE {
                    return Err(NetError::BodyTooLarge);
                }
                buf.extend_from_slice(&chunk[..size]);
            }
            Err(err)
                if err.kind() == ErrorKind::TimedOut || err.kind() == ErrorKind::WouldBlock =>
            {
                if buf.is_empty() {
                    return Err(NetError::Io(err));
                }
                break;
            }
            Err(err) => return Err(NetError::Io(err)),
        }
    }
    parse_http_response(&buf)
}

/// GET `url`, following at most [`MAX_REDIRECTS`] redirects.
///
/// Uses a new cookie jar for this call only. That jar is discarded with the
/// function, so cookies do not survive a later [`fetch`]. No initiator is passed,
/// so `SameSite` treats every hop as same-site. The shield runs before connect.
/// An `https` to `http` redirect is [`NetError::InsecureRedirectDowngrade`].
/// The returned URL is the final request URL, which may differ after redirects.
pub fn fetch(url: &str) -> Result<(String, Response), NetError> {
    let mut ephemeral_jar = CookieJar::new();
    fetch_with_cookies(url, &mut ephemeral_jar, None, true)
}

/// GET `url` with `jar`, following at most [`MAX_REDIRECTS`] redirects.
///
/// The shield is checked on every hop before [`http_get_with_cookie`] connects.
/// `Set-Cookie` from each response, including redirect responses, is stored in
/// `jar` before the next hop. `is_top_level_get` is forwarded to the cookie
/// header so `SameSite=Lax` can be included on a cross-site top-level GET.
/// A missing `Location` on a redirect status is [`NetError::InvalidResponse`].
/// A `Location` that does not resolve is [`NetError::InvalidUrl`]. Status codes
/// other than 301, 302, 303, 307, and 308 are returned as-is, including errors.
pub fn fetch_with_cookies(
    url: &str,
    jar: &mut CookieJar,
    initiator_url: Option<&str>,
    is_top_level_get: bool,
) -> Result<(String, Response), NetError> {
    let mut current = url.trim().to_string();
    for _ in 0..=MAX_REDIRECTS {
        let parsed = parse_url(&current)?;
        // Block before DNS and TCP. A shield hit never reaches http_get_with_cookie.
        if let Some(reason) = is_url_blocked_by_default_shield(&current, initiator_url) {
            return Err(NetError::BlockedByShield(reason.to_string()));
        }
        let cookie_hdr = jar.http_cookie_header(&current, initiator_url, is_top_level_get);
        let response = http_get_with_cookie(&parsed, cookie_hdr.as_deref())?;
        for set_cookie in response.headers_named("set-cookie") {
            jar.store_set_cookie(&current, set_cookie);
        }
        if matches!(response.status, 301 | 302 | 303 | 307 | 308) {
            let location = response
                .header("location")
                .ok_or(NetError::InvalidResponse)?;
            let next = resolve_url(&current, location).ok_or(NetError::InvalidUrl)?;
            let next_parsed = parse_url(&next)?;
            // Refuse an https to http downgrade. http to https is allowed.
            if parsed.scheme == "https" && next_parsed.scheme == "http" {
                return Err(NetError::InsecureRedirectDowngrade);
            }
            current = next;
            continue;
        }
        return Ok((current, response));
    }
    // The loop bound is the first response plus MAX_REDIRECTS hops.
    Err(NetError::TooManyRedirects)
}

/// POST `url`, following at most [`MAX_REDIRECTS`] redirects.
///
/// Uses a new cookie jar for this call only. 301, 302, and 303 switch the next
/// hop to GET and drop the body. 307 and 308 send the same body again. An
/// `https` to `http` redirect is refused. The shield runs before each connect.
pub fn fetch_post(
    url: &str,
    content_type: &str,
    body: &[u8],
) -> Result<(String, Response), NetError> {
    let mut ephemeral_jar = CookieJar::new();
    fetch_post_with_cookies(url, content_type, body, &mut ephemeral_jar, None)
}

/// POST `url` with `jar`, following at most [`MAX_REDIRECTS`] redirects.
///
/// The shield is checked before every connect. `Set-Cookie` is stored before
/// the next hop. 301, 302, and 303 set the next method to GET. On those GET
/// hops the cookie header is built as a top-level GET, so cross-site
/// `SameSite=Lax` cookies are included. 307 and 308 keep POST, and the cookie
/// header is not treated as a top-level GET. The original body is reused for
/// repeated POST hops. It is not sent after the method switches to GET.
pub fn fetch_post_with_cookies(
    url: &str,
    content_type: &str,
    body: &[u8],
    jar: &mut CookieJar,
    initiator_url: Option<&str>,
) -> Result<(String, Response), NetError> {
    let mut current = url.trim().to_string();
    let mut use_post = true;
    for _ in 0..=MAX_REDIRECTS {
        let parsed = parse_url(&current)?;
        // Block before DNS and TCP. A shield hit never reaches the POST or GET exchange.
        if let Some(reason) = is_url_blocked_by_default_shield(&current, initiator_url) {
            return Err(NetError::BlockedByShield(reason.to_string()));
        }
        // After a 301, 302, or 303, !use_post is true, so Lax cookies are sent.
        let cookie_hdr = jar.http_cookie_header(&current, initiator_url, !use_post);
        let response = if use_post {
            http_post_with_cookie(&parsed, content_type, body, cookie_hdr.as_deref())?
        } else {
            http_get_with_cookie(&parsed, cookie_hdr.as_deref())?
        };
        for set_cookie in response.headers_named("set-cookie") {
            jar.store_set_cookie(&current, set_cookie);
        }
        if matches!(response.status, 301 | 302 | 303 | 307 | 308) {
            // 301, 302, and 303 drop the POST body. 307 and 308 keep it.
            if matches!(response.status, 301 | 302 | 303) {
                use_post = false;
            }
            let location = response
                .header("location")
                .ok_or(NetError::InvalidResponse)?;
            let next = resolve_url(&current, location).ok_or(NetError::InvalidUrl)?;
            let next_parsed = parse_url(&next)?;
            // Refuse an https to http downgrade. http to https is allowed.
            if parsed.scheme == "https" && next_parsed.scheme == "http" {
                return Err(NetError::InsecureRedirectDowngrade);
            }
            current = next;
            continue;
        }
        return Ok((current, response));
    }
    // The loop bound is the first response plus MAX_REDIRECTS hops.
    Err(NetError::TooManyRedirects)
}

/// Replace any existing query with `query`.
///
/// The fragment is not handled. An empty `query` returns the URL with no `?`.
/// `query` is not encoded.
pub fn append_query(url: &str, query: &str) -> String {
    let base = url.split('?').next().unwrap_or(url);
    if query.is_empty() {
        base.to_string()
    } else {
        format!("{base}?{query}")
    }
}

/// Build an HTTP/1.1 GET. `Connection: close` is always set. No `Referer` is sent.
///
/// An empty or missing cookie header omits the `Cookie` line. The caller is
/// responsible for cookie flags. This function does not inspect them.
fn build_request_with_cookie(url: &Url, cookie_header: Option<&str>) -> String {
    let path = if url.path.is_empty() {
        "/"
    } else {
        url.path.as_str()
    };
    let query = url
        .query
        .as_ref()
        .map(|query| format!("?{query}"))
        .unwrap_or_default();
    let host = if Url::default_port(&url.scheme) == Some(url.port) {
        url.host.clone()
    } else {
        format!("{}:{}", url.host, url.port)
    };
    let cookie_line = match cookie_header {
        Some(c) if !c.is_empty() => format!("Cookie: {c}\r\n"),
        _ => String::new(),
    };
    format!(
        "GET {path}{query} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: flux-browser/0.18\r\nAccept: text/html,text/css,image/webp,image/png,image/jpeg,image/gif,image/x-icon,image/bmp,*/*\r\nAccept-Encoding: gzip, deflate, identity\r\nReferrer-Policy: no-referrer\r\nDNT: 1\r\nX-Content-Type-Options: nosniff\r\n{cookie_line}Connection: close\r\n\r\n"
    )
}

/// POST request bytes with no `Cookie` header. The body is copied after the header.
pub fn build_post_request(url: &Url, content_type: &str, body: &[u8]) -> Vec<u8> {
    build_post_request_with_cookie(url, content_type, body, None)
}

/// POST request bytes. `Connection: close` is always set. No `Referer` is sent.
///
/// CR, LF, and other control characters are removed from `content_type` so the
/// value cannot add another header. `Content-Length` is `body.len()`. An empty
/// or missing cookie header omits the `Cookie` line.
pub fn build_post_request_with_cookie(
    url: &Url,
    content_type: &str,
    body: &[u8],
    cookie_header: Option<&str>,
) -> Vec<u8> {
    let path = if url.path.is_empty() {
        "/"
    } else {
        url.path.as_str()
    };
    let query = url
        .query
        .as_ref()
        .map(|query| format!("?{query}"))
        .unwrap_or_default();
    let host = if Url::default_port(&url.scheme) == Some(url.port) {
        url.host.clone()
    } else {
        format!("{}:{}", url.host, url.port)
    };
    // Strip controls so Content-Type cannot inject another header line.
    let safe_ct: String = content_type
        .chars()
        .filter(|&c| c != '\r' && c != '\n' && !c.is_control())
        .collect();
    let cookie_line = match cookie_header {
        Some(c) if !c.is_empty() => format!("Cookie: {c}\r\n"),
        _ => String::new(),
    };
    let mut head = format!(
        "POST {path}{query} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: flux-browser/0.18\r\nAccept: text/html,text/css,image/webp,image/png,image/jpeg,image/gif,image/x-icon,image/bmp,*/*\r\nAccept-Encoding: gzip, deflate, identity\r\nReferrer-Policy: no-referrer\r\nDNT: 1\r\nX-Content-Type-Options: nosniff\r\nContent-Type: {safe_ct}\r\nContent-Length: {}\r\n{cookie_line}Connection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    head.extend_from_slice(body);
    head
}

#[cfg(test)]
mod tests {
    //! URL parsing, response decoding, cookie rules, the pre-connect shield,
    //! HSTS upgrades, and SHA-256 integrity checks. The shield case expects
    //! [`fetch`] to refuse a blocked host before a document is returned.

    use super::*;

    #[test]
    fn parses_http_and_https_urls() {
        let http = parse_url("http://example.com/a/b?x=1").unwrap();
        assert_eq!(http.scheme, "http");
        assert_eq!(http.host, "example.com");
        assert_eq!(http.port, 80);
        assert_eq!(http.path, "/a/b");
        assert_eq!(http.query.as_deref(), Some("x=1"));

        let https = parse_url("https://example.com/secure").unwrap();
        assert_eq!(https.scheme, "https");
        assert_eq!(https.port, 443);
        assert_eq!(https.path, "/secure");
    }

    #[test]
    fn refuses_unknown_schemes() {
        assert!(matches!(
            parse_url("ftp://example.com/"),
            Err(NetError::UnsupportedScheme)
        ));
    }

    #[test]
    fn parses_a_response_and_chunked_body() {
        let raw =
            b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 5\r\n\r\nhelloEXTRA";
        let response = parse_http_response(raw).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.reason, "OK");
        assert_eq!(response.body, b"hello");
        assert_eq!(response.header("Content-Type"), Some("text/html"));

        let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nFlux\r\n7\r\nBrowser\r\n0\r\n\r\n";
        let decoded = parse_http_response(chunked).unwrap();
        assert_eq!(decoded.body, b"FluxBrowser");

        let gz_payload = flux_compress::compress_gzip(b"<h1>Compressed Flux</h1>");
        let mut gz_http = format!(
            "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\n\r\n",
            gz_payload.len()
        )
        .into_bytes();
        gz_http.extend_from_slice(&gz_payload);
        let gz_resp = parse_http_response(&gz_http).unwrap();
        assert_eq!(gz_resp.body, b"<h1>Compressed Flux</h1>");
    }

    #[test]
    fn resolves_relative_http_https_and_file_links() {
        assert_eq!(
            resolve_url("http://example.com/docs/intro.html", "guide.html"),
            Some("http://example.com/docs/guide.html".to_string())
        );
        assert_eq!(
            resolve_url("https://example.com/app/index.html", "/assets/site.css"),
            Some("https://example.com/assets/site.css".to_string())
        );
        assert_eq!(
            resolve_url("http://example.com:8080/a/b/c.html", "../index.html?x=1"),
            Some("http://example.com:8080/a/index.html?x=1".to_string())
        );
        assert_eq!(
            resolve_url("examples/hello.html", "second.html"),
            Some("examples/second.html".to_string())
        );
        assert_eq!(resolve_url("examples/hello.html", "#top"), None);
        assert_eq!(resolve_url("examples/hello.html", "/etc/passwd"), None);
        assert_eq!(
            resolve_url("examples/hello.html", "javascript:alert(1)"),
            None
        );
    }

    #[test]
    fn builds_post_requests_and_appends_query_strings() {
        let url = parse_url("http://example.com/submit").unwrap();
        let req = build_post_request(&url, "application/x-www-form-urlencoded", b"q=flux&page=2");
        let text = String::from_utf8(req).unwrap();
        assert!(text.starts_with("POST /submit HTTP/1.1\r\n"));
        assert!(text.contains("Content-Type: application/x-www-form-urlencoded\r\n"));
        assert!(text.contains("Content-Length: 13\r\n"));
        assert!(text.ends_with("\r\n\r\nq=flux&page=2"));
        assert_eq!(
            append_query("http://example.com/search?old=1", "q=flux"),
            "http://example.com/search?q=flux"
        );
    }

    #[test]
    fn enforces_same_origin_policy_and_blocks_phishing_or_injection_urls() {
        assert!(matches!(
            parse_url("https://trusted.example.com@evil.example.com/login"),
            Err(NetError::InvalidUrl)
        ));
        assert!(matches!(
            parse_url("http://example.com/path\r\nX-Injected: 1"),
            Err(NetError::InvalidUrl)
        ));
        assert_eq!(
            resolve_url("https://example.com/index.html", "flux://security"),
            None
        );
        assert_eq!(
            resolve_url("https://example.com/index.html", "file:///etc/passwd"),
            None
        );
        assert_eq!(
            resolve_url("flux://home", "flux://security"),
            Some("flux://security".to_string())
        );
        assert!(is_same_origin(
            "https://example.com/app/index.html",
            "/api/data"
        ));
        assert!(is_same_origin(
            "https://example.com/app/index.html",
            "https://example.com/other"
        ));
        assert!(!is_same_origin(
            "https://example.com/app/index.html",
            "https://attacker.example.com/steal"
        ));
        assert!(!is_same_origin(
            "https://example.com/app/index.html",
            "http://example.com/app/index.html"
        ));
    }

    #[test]
    fn enforces_csp_corp_and_cookie_jar_security_policies() {
        let csp = ContentSecurityPolicy::parse(
            "default-src 'self'; img-src 'self' *.cdn.example.com; script-src 'self'",
        );
        assert!(!csp.allows_inline_script());
        assert!(csp.allows_resource(
            CspDirective::ScriptSrc,
            "https://example.com/index.html",
            "/app.js"
        ));
        assert!(!csp.allows_resource(
            CspDirective::ScriptSrc,
            "https://example.com/index.html",
            "https://evil.example.org/payload.js"
        ));
        assert!(csp.allows_resource(
            CspDirective::ImgSrc,
            "https://example.com/index.html",
            "https://static.cdn.example.com/logo.png"
        ));

        assert!(allows_corp_subresource(
            Some("same-origin"),
            "https://example.com/index.html",
            "https://example.com/img.png"
        ));
        assert!(!allows_corp_subresource(
            Some("same-origin"),
            "https://example.com/index.html",
            "https://sub.example.com/img.png"
        ));
        assert!(allows_corp_subresource(
            Some("same-site"),
            "https://example.com/index.html",
            "https://sub.example.com/img.png"
        ));

        let mut jar = CookieJar::new();
        assert!(jar.store_set_cookie(
            "https://example.com/login",
            "__Host-sid=abc123; Secure; HttpOnly; Path=/; SameSite=Strict"
        ));
        assert!(jar.store_set_cookie(
            "https://example.com/login",
            "theme=dark; Secure; Path=/; SameSite=Lax"
        ));
        // Reject __Host- cookie with Domain attribute or over HTTP
        assert!(!jar.store_set_cookie(
            "https://example.com/login",
            "__Host-bad=1; Secure; Domain=example.com; Path=/"
        ));
        assert!(
            !jar.store_set_cookie("http://example.com/login", "__Secure-tok=1; Secure; Path=/")
        );

        assert_eq!(
            jar.http_cookie_header(
                "https://example.com/dashboard",
                Some("https://example.com/login"),
                true
            ),
            Some("__Host-sid=abc123; theme=dark".to_string())
        );
        // Cross-site top-level GET omits SameSite=Strict (__Host-sid), includes SameSite=Lax (theme)
        assert_eq!(
            jar.http_cookie_header(
                "https://example.com/dashboard",
                Some("https://other.org/link"),
                true
            ),
            Some("theme=dark".to_string())
        );
        // Script cookie string strictly omits HttpOnly cookies (__Host-sid)
        assert_eq!(
            jar.script_cookie_string("https://example.com/dashboard"),
            "theme=dark"
        );

        let exported = jar.to_json();
        let restored = CookieJar::from_json(&exported).unwrap();
        assert_eq!(restored, jar);
    }

    #[test]
    fn blocks_tracker_ad_and_malware_domains_before_opening_socket() {
        let mut shield = NetworkShield::new();
        assert!(
            shield
                .check_and_count(
                    "https://stats.doubleclick.net/pixel.gif",
                    Some("https://example.com")
                )
                .is_some()
        );
        assert!(
            shield
                .check_and_count(
                    "https://telemetry.vendor.org/collect",
                    Some("https://example.com")
                )
                .is_some()
        );
        assert!(
            shield
                .check_and_count(
                    "https://cdn.other.org/fingerprint.js",
                    Some("https://example.com")
                )
                .is_some()
        );
        assert_eq!(shield.blocked_total(), 3);
        assert!(
            shield
                .check_and_count(
                    "https://example.com/assets/app.css",
                    Some("https://example.com")
                )
                .is_none()
        );
        assert!(matches!(
            fetch("https://adserver.example.net/banner.js"),
            Err(NetError::BlockedByShield(_))
        ));
    }

    #[test]
    fn enforces_hsts_upgrades_and_verifies_subresource_integrity_sha256() {
        let mut hsts = HstsStore::new();
        // Preloaded domain automatically upgrades HTTP -> HTTPS
        assert_eq!(
            hsts.upgrade_url_if_hsts("http://github.com/buntatoes"),
            "https://github.com/buntatoes"
        );
        // Ignore HSTS header sent over insecure HTTP
        assert!(!hsts.record_hsts_header(
            "http://bank.example.com/login",
            "max-age=31536000; includeSubDomains"
        ));
        // Accept HSTS header over HTTPS
        assert!(hsts.record_hsts_header(
            "https://bank.example.com/login",
            "max-age=31536000; includeSubDomains"
        ));
        assert_eq!(
            hsts.upgrade_url_if_hsts("http://api.bank.example.com/v1/status"),
            "https://api.bank.example.com/v1/status"
        );
        let hsts_json = hsts.to_json();
        let restored = HstsStore::from_json(&hsts_json).expect("valid HSTS JSON");
        assert_eq!(restored, hsts);

        let script_bytes = b"let verified = 1;";
        let token = sri_sha256_token(script_bytes);
        assert!(token.starts_with("sha256-"));
        assert!(verify_subresource_integrity(Some(&token), script_bytes));
        assert!(!verify_subresource_integrity(
            Some(&token),
            b"let verified = 0;"
        ));
        assert!(verify_subresource_integrity(None, script_bytes));
    }
}
