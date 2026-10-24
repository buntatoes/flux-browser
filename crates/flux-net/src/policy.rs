//! Content Security Policy, Cross-Origin Resource Policy, cookies, HSTS, and SHA-256 SRI.
//!
//! This module does not open sockets and does not speak TLS. [`crate::fetch`]
//! and [`crate::fetch_post`] use [`CookieJar`] while following redirects.
//! `flux-browser` also keeps a [`CookieJar`] and an [`HstsStore`] on the
//! session and calls [`ContentSecurityPolicy`], [`allows_corp_subresource`],
//! and [`verify_subresource_integrity`] when it loads a page.
//! Snapshots use `flux-json` with a depth cap of 16 and a 1 MiB cap.
//!
//! Limits: [`MAX_COOKIES_TOTAL`], [`MAX_COOKIES_PER_HOST`], [`MAX_COOKIE_BYTES`],
//! and [`MAX_HSTS_HOSTS`]. There is no public-suffix list. Same-site is the
//! scheme plus the last two hostname labels. HSTS `max-age` is stored but not
//! compared to a clock. Preloaded HSTS hosts are fixed in this file.

use crate::{is_same_origin, parse_url, resolve_url};
use flux_json::{JsonValue, parse_json_bounded, to_json_string};

/// Maximum cookies kept in one [`CookieJar`].
///
/// When a new cookie would exceed this, the oldest entry is dropped.
pub const MAX_COOKIES_TOTAL: usize = 256;

/// Maximum cookies kept for one domain string.
///
/// When a new cookie would exceed this, the oldest cookie with that domain is dropped.
pub const MAX_COOKIES_PER_HOST: usize = 32;

/// Maximum byte length of one `Set-Cookie` header value.
///
/// A longer value is rejected and not stored.
pub const MAX_COOKIE_BYTES: usize = 4096;

/// Fetch directive names this parser keeps.
///
/// Any other CSP directive name is ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CspDirective {
    /// `default-src`. Also the fallback when a specific list is empty.
    DefaultSrc,
    /// `script-src`.
    ScriptSrc,
    /// `style-src`.
    StyleSrc,
    /// `img-src`.
    ImgSrc,
    /// `media-src`.
    MediaSrc,
    /// `connect-src`.
    ConnectSrc,
}

/// One source expression from a CSP directive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CspSource {
    /// `'none'`. Denies the directive even if other sources are present.
    None,
    /// `'self'`. Same scheme, host, and port as the page.
    SelfOrigin,
    /// `'unsafe-inline'`. Consulted only by the inline script and style checks.
    UnsafeInline,
    /// A scheme token such as `https:` stored without the trailing colon.
    Scheme(String),
    /// A host, or a full origin when the token contains `://`.
    Host(String),
    /// `*` or a `*.suffix` host. The suffix itself also matches.
    WildcardSubdomain(String),
}

/// Parsed `Content-Security-Policy` header.
///
/// Only `default-src`, `script-src`, `style-src`, `img-src`, `media-src`, and
/// `connect-src` are stored. A repeated directive replaces the earlier list.
/// An empty list means that directive was absent. Resource and inline checks
/// then fall back to `default-src`. If that is also empty, the check allows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContentSecurityPolicy {
    /// `default-src` sources. Fallback when a specific list above is empty.
    pub default_src: Vec<CspSource>,
    /// `script-src` sources. Empty means use `default_src`.
    pub script_src: Vec<CspSource>,
    /// `style-src` sources. Empty means use `default_src`.
    pub style_src: Vec<CspSource>,
    /// `img-src` sources. Empty means use `default_src`.
    pub img_src: Vec<CspSource>,
    /// `media-src` sources. Empty means use `default_src`.
    pub media_src: Vec<CspSource>,
    /// `connect-src` sources. Empty means use `default_src`.
    pub connect_src: Vec<CspSource>,
}

impl ContentSecurityPolicy {
    /// Parse a `Content-Security-Policy` header.
    ///
    /// Directives are split on `;`. The first token is the name. Remaining
    /// tokens are sources. Unknown directive names are dropped. An empty
    /// header yields empty lists, which allow every check.
    pub fn parse(header: &str) -> Self {
        let mut csp = Self::default();
        for raw_directive in header.split(';') {
            let trimmed = raw_directive.trim();
            if trimmed.is_empty() {
                continue;
            }
            let mut tokens = trimmed.split_whitespace();
            let Some(name) = tokens.next() else {
                continue;
            };
            let sources: Vec<CspSource> = tokens.map(parse_csp_source).collect();
            match name.to_ascii_lowercase().as_str() {
                "default-src" => csp.default_src = sources,
                "script-src" => csp.script_src = sources,
                "style-src" => csp.style_src = sources,
                "img-src" => csp.img_src = sources,
                "media-src" => csp.media_src = sources,
                "connect-src" => csp.connect_src = sources,
                _ => {}
            }
        }
        csp
    }

    /// Specific directive list, or `default-src` when that list is empty.
    fn effective_sources(&self, directive: CspDirective) -> &[CspSource] {
        let specific = match directive {
            CspDirective::DefaultSrc => &self.default_src,
            CspDirective::ScriptSrc => &self.script_src,
            CspDirective::StyleSrc => &self.style_src,
            CspDirective::ImgSrc => &self.img_src,
            CspDirective::MediaSrc => &self.media_src,
            CspDirective::ConnectSrc => &self.connect_src,
        };
        if !specific.is_empty() {
            specific
        } else {
            &self.default_src
        }
    }

    /// Whether inline script is allowed.
    ///
    /// An empty effective list allows. `'none'` denies, including when
    /// `'unsafe-inline'` is also present. Otherwise `'unsafe-inline'` is required.
    pub fn allows_inline_script(&self) -> bool {
        let sources = self.effective_sources(CspDirective::ScriptSrc);
        if sources.is_empty() {
            return true;
        }
        if sources.contains(&CspSource::None) {
            return false;
        }
        sources.contains(&CspSource::UnsafeInline)
    }

    /// Whether inline style is allowed. Same rule as [`Self::allows_inline_script`] for `style-src`.
    pub fn allows_inline_style(&self) -> bool {
        let sources = self.effective_sources(CspDirective::StyleSrc);
        if sources.is_empty() {
            return true;
        }
        if sources.contains(&CspSource::None) {
            return false;
        }
        sources.contains(&CspSource::UnsafeInline)
    }

    /// Whether `directive` allows `target_url` from `page_url`.
    ///
    /// An empty effective list allows. `'none'` denies the whole directive.
    /// `'unsafe-inline'` does not match a URL. Other sources are tried in order
    /// and the first match allows. No match denies.
    /// `target_url` is resolved against `page_url` when [`resolve_url`] accepts it.
    pub fn allows_resource(
        &self,
        directive: CspDirective,
        page_url: &str,
        target_url: &str,
    ) -> bool {
        let sources = self.effective_sources(directive);
        if sources.is_empty() {
            return true;
        }
        if sources.contains(&CspSource::None) {
            return false;
        }
        let resolved = resolve_url(page_url, target_url).unwrap_or_else(|| target_url.to_string());
        for src in sources {
            match src {
                // 'none' already denied above. 'unsafe-inline' never matches a URL.
                CspSource::None | CspSource::UnsafeInline => {}
                CspSource::SelfOrigin => {
                    if is_same_origin(page_url, &resolved) {
                        return true;
                    }
                }
                CspSource::Scheme(scheme) => {
                    if let Ok(parsed) = parse_url(&resolved) {
                        if parsed.scheme.eq_ignore_ascii_case(scheme) {
                            return true;
                        }
                    } else if resolved
                        .to_ascii_lowercase()
                        .starts_with(&format!("{scheme}:"))
                    {
                        return true;
                    }
                }
                CspSource::WildcardSubdomain(suffix) => {
                    // `*` allows the target without parsing it.
                    if suffix == "*" {
                        return true;
                    }
                    if let Ok(parsed) = parse_url(&resolved) {
                        if parsed.host.eq_ignore_ascii_case(suffix)
                            || parsed
                                .host
                                .to_ascii_lowercase()
                                .ends_with(&format!(".{suffix}"))
                        {
                            return true;
                        }
                    }
                }
                CspSource::Host(host_expr) => {
                    // A token that contains `://` must match scheme, host, and port.
                    // Any other host token must match the target host exactly.
                    if host_expr.contains("://") {
                        if is_same_origin(host_expr, &resolved) {
                            return true;
                        }
                    } else if let Ok(parsed) = parse_url(&resolved) {
                        if parsed.host.eq_ignore_ascii_case(host_expr) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }
}

/// Map one CSP source token to [`CspSource`].
///
/// Recognizes `'none'`, `'self'`, `'unsafe-inline'`, `*`, a scheme ending in
/// `:`, and `*.suffix`. Every other token, including an unknown quoted keyword,
/// becomes [`CspSource::Host`] with surrounding quotes removed. Nothing fails
/// the parse. Matching happens later in [`ContentSecurityPolicy::allows_resource`].
fn parse_csp_source(token: &str) -> CspSource {
    let clean = token.trim().to_ascii_lowercase();
    match clean.as_str() {
        "'none'" => CspSource::None,
        "'self'" => CspSource::SelfOrigin,
        "'unsafe-inline'" => CspSource::UnsafeInline,
        "*" => CspSource::WildcardSubdomain("*".to_string()),
        _ if clean.ends_with(':') && !clean.contains('/') => {
            CspSource::Scheme(clean.trim_end_matches(':').to_string())
        }
        _ if clean.starts_with("*.") => {
            CspSource::WildcardSubdomain(clean.trim_start_matches("*.").to_string())
        }
        _ => CspSource::Host(clean.trim_matches('\'').to_string()),
    }
}

/// Whether CORP allows a subresource.
///
/// A missing header, an empty value, and `cross-origin` allow. `same-origin`
/// requires the same scheme, host, and port. `same-site` uses [`is_same_site`].
/// Any other token denies.
pub fn allows_corp_subresource(
    corp_header: Option<&str>,
    page_url: &str,
    resource_url: &str,
) -> bool {
    let Some(raw) = corp_header else {
        return true;
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "same-origin" => is_same_origin(page_url, resource_url),
        "same-site" => is_same_site(page_url, resource_url),
        "cross-origin" | "" => true,
        _ => false,
    }
}

/// Whether two URLs share a scheme and a registrable domain.
///
/// Same origin is same-site. Otherwise both URLs must parse, the schemes must
/// be equal, and the last two hostname labels must match. A URL that does not parse
/// is not same-site unless [`is_same_origin`] already returned true.
pub fn is_same_site(a_url: &str, b_url: &str) -> bool {
    if is_same_origin(a_url, b_url) {
        return true;
    }
    let (Ok(a), Ok(b)) = (parse_url(a_url), parse_url(b_url)) else {
        return false;
    };
    if a.scheme != b.scheme {
        return false;
    }
    registrable_domain(&a.host) == registrable_domain(&b.host)
}

/// Last two dot-separated labels, lowercased.
///
/// A host with fewer than two labels is returned lowercased as a whole.
/// This is not a public-suffix list: `example.co.uk` yields `co.uk`.
fn registrable_domain(host: &str) -> String {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() >= 2 {
        format!("{}.{}", parts[parts.len() - 2], parts[parts.len() - 1]).to_ascii_lowercase()
    } else {
        host.to_ascii_lowercase()
    }
}

/// `SameSite` attribute stored on a cookie.
///
/// A missing or unrecognized attribute is stored as [`SameSite::Lax`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SameSite {
    /// Sent only when the initiator and the target are same-site.
    Strict,
    /// Sent on same-site requests, and on cross-site top-level GET requests.
    Lax,
    /// Sent on cross-site requests only when the cookie is `Secure` and the target is `https`.
    None,
}

impl SameSite {
    /// Canonical attribute spelling used in JSON snapshots.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Strict => "Strict",
            Self::Lax => "Lax",
            Self::None => "None",
        }
    }

    /// Parse a `SameSite` attribute.
    ///
    /// `strict` and `none` are recognized. Every other value, including an empty
    /// string, becomes [`SameSite::Lax`].
    pub fn from_str_attr(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "strict" => Self::Strict,
            "none" => Self::None,
            _ => Self::Lax,
        }
    }
}

/// One stored cookie.
///
/// `Expires` is not a field. Only `Max-Age` is read on input, and a positive
/// `Max-Age` is not stored, so nothing here expires by time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cookie {
    /// Cookie name, including any `__Host-` or `__Secure-` prefix.
    pub name: String,
    /// Cookie value. Not decoded.
    pub value: String,
    /// Host-only host, or the `Domain` attribute without a leading dot.
    pub domain: String,
    /// True when no `Domain` attribute was accepted. Match is exact host only.
    pub host_only: bool,
    /// Path prefix. Defaults to `/` when the attribute is absent or does not start with `/`.
    pub path: String,
    /// `Secure` flag. Sent only to `https`, and stored only from `https`.
    pub secure: bool,
    /// `HttpOnly` flag. Sent on the HTTP header and omitted from script access.
    pub http_only: bool,
    /// `SameSite` mode. Missing attributes are stored as [`SameSite::Lax`].
    pub same_site: SameSite,
}

/// Session cookie jar.
///
/// `Secure` is required to store a `Secure` cookie and to send it.
/// `HttpOnly` is stored and is included on the HTTP `Cookie` header, but
/// [`Self::script_cookie_string`] omits it. `SameSite=None` is rejected unless
/// `Secure` is set. `__Secure-` requires `Secure` on `https`. `__Host-` also
/// requires `Path=/` and no `Domain`. `Expires` is ignored. There is no
/// eviction by time, only by [`MAX_COOKIES_PER_HOST`] and [`MAX_COOKIES_TOTAL`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CookieJar {
    cookies: Vec<Cookie>,
}

impl CookieJar {
    /// Empty jar.
    pub fn new() -> Self {
        Self {
            cookies: Vec::new(),
        }
    }

    /// Number of stored cookies.
    pub fn len(&self) -> usize {
        self.cookies.len()
    }

    /// Whether the jar holds no cookies.
    pub fn is_empty(&self) -> bool {
        self.cookies.is_empty()
    }

    /// Drop every cookie.
    pub fn clear(&mut self) {
        self.cookies.clear();
    }

    /// Stored cookies in insertion order, after eviction of older entries.
    pub fn cookies(&self) -> &[Cookie] {
        &self.cookies
    }

    /// Parse and store one `Set-Cookie` value received from `request_url`.
    ///
    /// Returns `false` without changing the jar when the value is too long, contains
    /// a byte below 0x20 or 0x7F, the request URL does not parse, the name is empty
    /// or contains a cookie separator, `Secure` was set on a non-`https` response,
    /// `SameSite=None` lacks `Secure`, a `__Secure-` or `__Host-` prefix rule fails,
    /// or the `Domain` is not a suffix of the request host or has no dot.
    ///
    /// Returns `true` after deleting a match when `Max-Age` is zero or negative.
    /// A positive `Max-Age` does not set a lifetime. `Expires` is ignored.
    /// An existing name, domain, and path is updated in place. A `Secure` cookie
    /// is not updated from a non-`https` response.
    pub fn store_set_cookie(&mut self, request_url: &str, header_value: &str) -> bool {
        if header_value.len() > MAX_COOKIE_BYTES
            || header_value.bytes().any(|b| b < 0x20 || b == 0x7F)
        {
            return false;
        }
        let Ok(req) = parse_url(request_url) else {
            return false;
        };
        let is_https = req.scheme == "https";

        let mut parts = header_value.split(';');
        let Some(name_val) = parts.next() else {
            return false;
        };
        let Some((raw_name, raw_val)) = name_val.split_once('=') else {
            return false;
        };
        let name = raw_name.trim();
        let value = raw_val.trim();
        if name.is_empty()
            || name
                .bytes()
                .any(|b| matches!(b, b' ' | b'\t' | b';' | b',' | b'='))
        {
            return false;
        }

        let mut domain_attr: Option<String> = None;
        let mut path_attr: Option<String> = None;
        let mut secure = false;
        let mut http_only = false;
        let mut same_site = SameSite::Lax;
        let mut expired = false;

        for attr in parts {
            let trimmed = attr.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some((k, v)) = trimmed.split_once('=') {
                let key = k.trim().to_ascii_lowercase();
                let val = v.trim();
                match key.as_str() {
                    "domain" => {
                        let clean_dom = val.trim_start_matches('.').to_ascii_lowercase();
                        if !clean_dom.is_empty() {
                            domain_attr = Some(clean_dom);
                        }
                    }
                    "path" => {
                        if val.starts_with('/') {
                            path_attr = Some(val.to_string());
                        }
                    }
                    "samesite" => {
                        same_site = SameSite::from_str_attr(val);
                    }
                    "max-age" => {
                        if let Ok(seconds) = val.parse::<i64>() {
                            if seconds <= 0 {
                                expired = true;
                            }
                        }
                    }
                    _ => {}
                }
            } else {
                match trimmed.to_ascii_lowercase().as_str() {
                    "secure" => secure = true,
                    "httponly" => http_only = true,
                    _ => {}
                }
            }
        }

        // Secure cookies are accepted only from https. SameSite=None also requires Secure.
        if secure && !is_https {
            return false;
        }
        if same_site == SameSite::None && !secure {
            return false;
        }

        // __Secure- requires Secure on https. __Host- also forbids Domain and requires Path=/.
        if name.starts_with("__Secure-") && !(secure && is_https) {
            return false;
        }
        if name.starts_with("__Host-") {
            if !secure || !is_https || domain_attr.is_some() || path_attr.as_deref() != Some("/") {
                return false;
            }
        }

        let (domain, host_only) = match domain_attr {
            Some(dom) => {
                if !domain_matches(&req.host, &dom) || !dom.contains('.') {
                    return false;
                }
                (dom, false)
            }
            None => (req.host.clone(), true),
        };
        let path = path_attr.unwrap_or_else(|| "/".to_string());

        // Max-Age of 0 or less deletes the matching cookie and does not store a new one.
        if expired {
            self.cookies
                .retain(|c| !(c.name == name && c.domain == domain && c.path == path));
            return true;
        }

        if let Some(existing) = self
            .cookies
            .iter_mut()
            .find(|c| c.name == name && c.domain == domain && c.path == path)
        {
            // A Secure cookie cannot be overwritten from a non-https response.
            if existing.secure && !is_https {
                return false;
            }
            existing.value = value.to_string();
            existing.secure = secure;
            existing.http_only = http_only;
            existing.same_site = same_site;
            existing.host_only = host_only;
            return true;
        }

        // Drop the oldest cookie for this domain, then the oldest cookie in the jar.
        let host_count = self.cookies.iter().filter(|c| c.domain == domain).count();
        if host_count >= MAX_COOKIES_PER_HOST {
            if let Some(pos) = self.cookies.iter().position(|c| c.domain == domain) {
                self.cookies.remove(pos);
            }
        }
        if self.cookies.len() >= MAX_COOKIES_TOTAL {
            self.cookies.remove(0);
        }

        self.cookies.push(Cookie {
            name: name.to_string(),
            value: value.to_string(),
            domain,
            host_only,
            path,
            secure,
            http_only,
            same_site,
        });
        true
    }

    /// `Cookie` header value for `target_url`, including `HttpOnly` cookies.
    ///
    /// `None` when the URL does not parse or no cookie matches. `Secure` cookies
    /// are omitted on non-`https` targets. Host-only cookies require an exact host.
    /// Domain cookies match the host exactly or as a dotted suffix. The path must match. `SameSite=Strict`
    /// is omitted when the initiator is cross-site. `SameSite=Lax` is omitted on a
    /// cross-site request that is not a top-level GET. `SameSite=None` is omitted
    /// unless the target is `https`. A missing initiator is treated as same-site.
    pub fn http_cookie_header(
        &self,
        target_url: &str,
        initiator_url: Option<&str>,
        is_top_level_get: bool,
    ) -> Option<String> {
        self.matching_cookie_string(target_url, initiator_url, is_top_level_get, true)
    }

    /// Script-visible cookie string for `page_url`.
    ///
    /// `HttpOnly` cookies are omitted. The initiator is the page itself and the
    /// request is treated as a top-level GET, so `SameSite` does not hide cookies
    /// here. An unparseable URL or no match yields an empty string, not `None`.
    pub fn script_cookie_string(&self, page_url: &str) -> String {
        self.matching_cookie_string(page_url, Some(page_url), true, false)
            .unwrap_or_default()
    }

    /// Join matching `name=value` pairs with `"; "`.
    ///
    /// `include_http_only` is true for the HTTP header and false for script access.
    fn matching_cookie_string(
        &self,
        target_url: &str,
        initiator_url: Option<&str>,
        is_top_level_get: bool,
        include_http_only: bool,
    ) -> Option<String> {
        let Ok(target) = parse_url(target_url) else {
            return None;
        };
        let is_https = target.scheme == "https";
        let same_site_req = match initiator_url {
            Some(init) => is_same_site(init, target_url),
            None => true,
        };

        let mut pairs = Vec::new();
        for cookie in &self.cookies {
            if !include_http_only && cookie.http_only {
                continue;
            }
            if cookie.secure && !is_https {
                continue;
            }
            if cookie.host_only {
                if !target.host.eq_ignore_ascii_case(&cookie.domain) {
                    continue;
                }
            } else if !domain_matches(&target.host, &cookie.domain) {
                continue;
            }
            if !path_matches(&target.path, &cookie.path) {
                continue;
            }
            // Strict never leaves the site. Lax leaves only on a top-level GET.
            // None is sent only to https. A missing initiator counts as same-site.
            match cookie.same_site {
                SameSite::Strict if !same_site_req => continue,
                SameSite::Lax if !same_site_req && !is_top_level_get => continue,
                SameSite::None if !is_https => continue,
                _ => {}
            }
            pairs.push(format!("{}={}", cookie.name, cookie.value));
        }

        if pairs.is_empty() {
            None
        } else {
            Some(pairs.join("; "))
        }
    }

    /// Serialize every stored cookie. Flags are written as stored. No re-check.
    pub fn to_json(&self) -> String {
        let mut root = JsonValue::empty_object();
        let mut list = JsonValue::empty_array();
        for c in &self.cookies {
            let mut item = JsonValue::empty_object();
            item.insert("name", JsonValue::String(c.name.clone()));
            item.insert("value", JsonValue::String(c.value.clone()));
            item.insert("domain", JsonValue::String(c.domain.clone()));
            item.insert("host_only", JsonValue::Bool(c.host_only));
            item.insert("path", JsonValue::String(c.path.clone()));
            item.insert("secure", JsonValue::Bool(c.secure));
            item.insert("http_only", JsonValue::Bool(c.http_only));
            item.insert(
                "same_site",
                JsonValue::String(c.same_site.as_str().to_string()),
            );
            list.push(item);
        }
        root.insert("cookies", list);
        to_json_string(&root)
    }

    /// Restore a jar from JSON produced by [`Self::to_json`].
    ///
    /// The document is capped at depth 16 and 1 MiB. Entries beyond
    /// [`MAX_COOKIES_TOTAL`] are ignored. A missing name, value, or domain skips
    /// that entry. Prefix, `Secure`, and `SameSite` rules are not applied again.
    /// A missing `same_site` becomes [`SameSite::Lax`]. A missing path becomes `/`.
    pub fn from_json(source: &str) -> Result<Self, String> {
        let doc = parse_json_bounded(source, 16, 1024 * 1024).map_err(|e| e.to_string())?;
        let items = doc.get_array("cookies").ok_or("missing cookies array")?;
        let mut jar = Self::new();
        for item in items.iter().take(MAX_COOKIES_TOTAL) {
            let Some(name) = item.get_str("name") else {
                continue;
            };
            let Some(value) = item.get_str("value") else {
                continue;
            };
            let Some(domain) = item.get_str("domain") else {
                continue;
            };
            let host_only = item.get_bool("host_only").unwrap_or(true);
            let path = item.get_str("path").unwrap_or("/").to_string();
            let secure = item.get_bool("secure").unwrap_or(false);
            let http_only = item.get_bool("http_only").unwrap_or(false);
            let same_site = item
                .get_str("same_site")
                .map(SameSite::from_str_attr)
                .unwrap_or(SameSite::Lax);
            jar.cookies.push(Cookie {
                name: name.to_string(),
                value: value.to_string(),
                domain: domain.to_string(),
                host_only,
                path,
                secure,
                http_only,
                same_site,
            });
        }
        Ok(jar)
    }
}

/// Host equals the cookie domain, or ends with `.{cookie_domain}`.
///
/// Comparison is ASCII case-insensitive. This does not reject a domain that
/// is a public suffix. Callers that store cookies also require a dot in the domain.
fn domain_matches(host: &str, cookie_domain: &str) -> bool {
    let host_low = host.to_ascii_lowercase();
    let dom_low = cookie_domain.to_ascii_lowercase();
    host_low == dom_low || host_low.ends_with(&format!(".{dom_low}"))
}

/// Request path equals the cookie path, or extends it on a `/` boundary.
///
/// A cookie path of `/docs` matches `/docs` and `/docs/a`, not `/docs2`.
/// A cookie path that already ends in `/` matches any longer path with that prefix.
fn path_matches(req_path: &str, cookie_path: &str) -> bool {
    if req_path == cookie_path {
        return true;
    }
    if req_path.starts_with(cookie_path) {
        return cookie_path.ends_with('/') || req_path[cookie_path.len()..].starts_with('/');
    }
    false
}

/// Maximum learned HSTS hosts in [`HstsStore`].
///
/// The preload list does not count toward this cap. A new host past the cap
/// drops the oldest learned entry. `max-age=0` removes one learned host and
/// does not remove a preloaded host.
pub const MAX_HSTS_HOSTS: usize = 512;

/// Hosts that [`HstsStore::requires_https`] always treats as HSTS, including subdomains.
/// These entries are not written by [`HstsStore::to_json`].
const PRELOADED_HSTS_DOMAINS: &[&str] = &[
    "github.com",
    "githubusercontent.com",
    "wikipedia.org",
    "eff.org",
    "cloudflare.com",
    "duckduckgo.com",
    "proton.me",
];

/// One learned HSTS host. Not used for the preload list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HstsEntry {
    /// Lowercased host the policy was learned for. Not a wildcard.
    pub host: String,
    /// When true, hosts ending in `.{host}` also require HTTPS.
    pub include_subdomains: bool,
    /// Stored `max-age` in seconds. Not compared to a clock.
    pub max_age: u64,
}

/// Learned HSTS policies plus the built-in preload list.
///
/// A `Strict-Transport-Security` header is stored only when the request URL is
/// `https`. `max-age` is required and is stored as a number. It is not compared
/// to a clock, so a positive age does not expire. `includeSubDomains` applies
/// only to learned entries. Preload hosts always include subdomains.
/// [`Self::upgrade_url_if_hsts`] rewrites `http://` to `https://` only when the
/// parsed port is 80.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HstsStore {
    entries: Vec<HstsEntry>,
}

impl HstsStore {
    /// Empty learned list. Preload still applies in [`Self::requires_https`].
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Number of learned hosts. Preload hosts are not included.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no learned hosts are stored. Preload can still force HTTPS.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drop learned hosts. Preload remains in effect.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Learned hosts in insertion order.
    pub fn entries(&self) -> &[HstsEntry] {
        &self.entries
    }

    /// Store a `Strict-Transport-Security` header from `request_url`.
    ///
    /// Returns `false` when the URL does not parse, the scheme is not `https`,
    /// or `max-age` is missing or not a `u64`. A header on `http` is ignored.
    /// `max-age=0` deletes the learned host and returns `true`. Unknown directives
    /// are ignored. `includeSubDomains` is recognized only as a flag token.
    pub fn record_hsts_header(&mut self, request_url: &str, header_value: &str) -> bool {
        let Ok(parsed) = parse_url(request_url) else {
            return false;
        };
        // An HSTS header received over http is ignored and does not change the store.
        if parsed.scheme != "https" {
            return false;
        }
        let mut max_age: Option<u64> = None;
        let mut include_subdomains = false;

        for directive in header_value.split(';') {
            let trimmed = directive.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some((k, v)) = trimmed.split_once('=') {
                if k.trim().eq_ignore_ascii_case("max-age") {
                    let clean_v = v.trim().trim_matches('"');
                    if let Ok(seconds) = clean_v.parse::<u64>() {
                        max_age = Some(seconds);
                    }
                }
            } else if trimmed.eq_ignore_ascii_case("includesubdomains") {
                include_subdomains = true;
            }
        }

        let Some(age) = max_age else {
            return false;
        };
        let host = parsed.host.to_ascii_lowercase();
        // max-age=0 removes the learned host only. It does not remove a preload match.
        if age == 0 {
            self.entries.retain(|e| e.host != host);
            return true;
        }

        if let Some(existing) = self.entries.iter_mut().find(|e| e.host == host) {
            existing.max_age = age;
            existing.include_subdomains = include_subdomains;
            return true;
        }

        // Cap learned hosts. The oldest learned entry is dropped.
        if self.entries.len() >= MAX_HSTS_HOSTS {
            self.entries.remove(0);
        }
        self.entries.push(HstsEntry {
            host,
            include_subdomains,
            max_age: age,
        });
        true
    }

    /// Whether `host` must use HTTPS.
    ///
    /// True when the host equals a preload domain or ends with `.{preload}`,
    /// equals a learned host, or ends with `.{learned}` and that entry set
    /// `includeSubDomains`. An empty host is false. `max-age` is not consulted.
    pub fn requires_https(&self, host: &str) -> bool {
        let clean = host.trim().to_ascii_lowercase();
        if clean.is_empty() {
            return false;
        }
        for &preload in PRELOADED_HSTS_DOMAINS {
            if clean == preload || clean.ends_with(&format!(".{preload}")) {
                return true;
            }
        }
        for entry in &self.entries {
            if clean == entry.host {
                return true;
            }
            if entry.include_subdomains && clean.ends_with(&format!(".{}", entry.host)) {
                return true;
            }
        }
        false
    }

    /// Rewrite an `http://` URL to `https://` when the host is HSTS and the port is 80.
    ///
    /// Any other scheme is returned trimmed and unchanged. An `http` URL that does
    /// not parse is unchanged. An HSTS host on a port other than 80 is unchanged,
    /// so a non-default port is not upgraded.
    pub fn upgrade_url_if_hsts(&self, url: &str) -> String {
        let trimmed = url.trim();
        let Some(rest) = trimmed.strip_prefix("http://") else {
            return trimmed.to_string();
        };
        let Ok(parsed) = parse_url(trimmed) else {
            return trimmed.to_string();
        };
        if self.requires_https(&parsed.host) {
            // Only the default http port is rewritten. The rest of the URL is kept.
            if parsed.port == 80 {
                format!("https://{rest}")
            } else {
                trimmed.to_string()
            }
        } else {
            trimmed.to_string()
        }
    }

    /// Serialize learned hosts. The preload list is not included.
    pub fn to_json(&self) -> String {
        let mut root = JsonValue::empty_object();
        let mut list = JsonValue::empty_array();
        for entry in &self.entries {
            let mut item = JsonValue::empty_object();
            item.insert("host", JsonValue::String(entry.host.clone()));
            item.insert(
                "include_subdomains",
                JsonValue::Bool(entry.include_subdomains),
            );
            item.insert("max_age", JsonValue::Number(entry.max_age as f64));
            list.push(item);
        }
        root.insert("hsts", list);
        to_json_string(&root)
    }

    /// Restore learned hosts from JSON produced by [`Self::to_json`].
    ///
    /// Depth cap 16 and 1 MiB. Entries past [`MAX_HSTS_HOSTS`] are ignored.
    /// A missing host skips the entry. A missing `max_age` becomes 31536000.
    /// A non-positive `max_age` becomes 1, so this path does not delete hosts.
    /// Preload is applied later by [`Self::requires_https`], not from this JSON.
    pub fn from_json(source: &str) -> Result<Self, String> {
        let doc = parse_json_bounded(source, 16, 1024 * 1024).map_err(|e| e.to_string())?;
        let items = doc.get_array("hsts").ok_or("missing hsts array")?;
        let mut store = Self::new();
        for item in items.iter().take(MAX_HSTS_HOSTS) {
            let Some(host) = item.get_str("host") else {
                continue;
            };
            let include_subdomains = item.get_bool("include_subdomains").unwrap_or(false);
            let max_age = item.get_i64("max_age").unwrap_or(31_536_000).max(1) as u64;
            store.entries.push(HstsEntry {
                host: host.to_ascii_lowercase(),
                include_subdomains,
                max_age,
            });
        }
        Ok(store)
    }
}

/// FIPS 180-4 SHA-256 round constants.
const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// SHA-256 over `data`.
///
/// Pads with `0x80`, then zeros until the length modulo 64 is 56, then the
/// bit length as a big-endian `u64`. The bit length wraps on overflow of that
/// multiply. Always returns 32 bytes. There is no failure value.
pub fn sha256_digest(data: &[u8]) -> [u8; 32] {
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity(data.len() + 72);
    padded.extend_from_slice(data);
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word_bytes) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word_bytes[0], word_bytes[1], word_bytes[2], word_bytes[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        let mut f = state[5];
        let mut g = state[6];
        let mut h = state[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }

    let mut out = [0u8; 32];
    for (i, word) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// `sha256-` plus standard base64 of [`sha256_digest`], with `=` padding.
pub fn sri_sha256_token(payload: &[u8]) -> String {
    let digest = sha256_digest(payload);
    format!("sha256-{}", base64_encode(&digest))
}

/// Check an `integrity` attribute against `payload`.
///
/// `None` and an empty or whitespace-only attribute allow the payload, because
/// no digest was required. When the attribute is present, only tokens that
/// start with `sha256-` are compared. Other algorithms are ignored. If no
/// `sha256-` token is present, the check fails. One matching token is enough.
pub fn verify_subresource_integrity(integrity_attr: Option<&str>, payload: &[u8]) -> bool {
    let Some(raw) = integrity_attr else {
        return true;
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return true;
    }
    let sha256_tokens: Vec<&str> = trimmed
        .split_whitespace()
        .filter(|tok| tok.starts_with("sha256-"))
        .collect();
    // An integrity value with no sha256 token does not pass.
    if sha256_tokens.is_empty() {
        return false;
    }
    let expected = sri_sha256_token(payload);
    sha256_tokens.contains(&expected.as_str())
}

/// Standard base64 with `=` padding. The alphabet is not URL-safe.
/// Used only to build SRI tokens.
fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}
