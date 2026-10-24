//! Tracker and malicious-host checks applied before a socket is opened.
//!
//! [`crate::fetch`] and [`crate::fetch_post`] call
//! [`is_url_blocked_by_default_shield`] after URL parsing and before
//! [`crate::http_get`] or [`crate::http_post`]. This module does not resolve
//! DNS, open TCP, or run TLS. It is not an HTTP client.
//!
//! A match is a blocked domain suffix, a blocked host prefix, or a tracking
//! path. An allow-list entry skips those rules. Turning the shield off, or
//! handing it a URL [`crate::parse_url`] rejects, returns no block reason.
//! Path patterns that are not named as a pixel, beacon, fingerprint, pagead,
//! ad server, or telemetry collector are blocked only when the initiator is
//! present and cross-site. `fetch` passes no initiator, so those path-only
//! rules do not fire on that path. Suffix and prefix rules still do.

use crate::{is_same_site, parse_url};

/// Hosts blocked as themselves or as a dotted suffix (`ads.example.com` blocks
/// `tracker.ads.example.com`). Compared after the host is lowercased.
const BLOCKED_DOMAIN_SUFFIXES: &[&str] = &[
    "doubleclick.net",
    "adnxs.com",
    "adsrvr.org",
    "adsystem.com",
    "criteo.com",
    "criteo.net",
    "taboola.com",
    "outbrain.com",
    "scorecardresearch.com",
    "quantserve.com",
    "hotjar.com",
    "mixpanel.com",
    "segment.io",
    "amplitude.com",
    "omtrdc.net",
    "demdex.net",
    "rubiconproject.com",
    "pubmatic.com",
    "openx.net",
    "casalemedia.com",
    "moatads.com",
    "chartbeat.com",
    "nr-data.net",
    "mouseflow.com",
    "fullstory.com",
    "clarity.ms",
    "adserver.example.net",
    "tracker.example.org",
    "telemetry.example.org",
    "malware-test.example.org",
    "phishing-test.example.org",
];

/// Prefixes matched against the start of the full hostname.
/// `ads.vendor.test` matches `ads.`. `myads.vendor.test` does not.
const BLOCKED_HOST_PREFIXES: &[&str] = &[
    "ads.",
    "ad.",
    "adservice.",
    "adserver.",
    "telemetry.",
    "tracker.",
    "tracking.",
    "pixel.",
    "beacon.",
    "metrics.",
    "analytics.",
    "fingerprinter.",
];

/// Substrings matched against the lowercased path.
/// `pagead`, `adserver`, `pixel`, `beacon`, `fingerprint`, and `telemetry`
/// block even for a same-site or missing initiator. `/ads/banner` and
/// `/track.js` block only when the initiator is cross-site.
const BLOCKED_PATH_PATTERNS: &[&str] = &[
    "/pagead/",
    "/adserver/",
    "/ads/banner",
    "/pixel.gif",
    "/beacon.js",
    "/fingerprint.js",
    "/telemetry/collect",
    "/track.js",
];

/// Why [`NetworkShield::inspect_url`] refused a URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShieldBlockReason {
    /// Host equals a blocked domain or ends with `.{domain}`.
    BlockedDomain(String),
    /// Host starts with a blocked tracker prefix such as `ads.` or `telemetry.`.
    BlockedTrackerSubdomain(String),
    /// Path contains a blocked tracking pattern.
    BlockedTrackingPath(String),
}

impl std::fmt::Display for ShieldBlockReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BlockedDomain(dom) => write!(f, "blocked tracker/ad domain ({dom})"),
            Self::BlockedTrackerSubdomain(host) => {
                write!(f, "blocked tracker/telemetry subdomain ({host})")
            }
            Self::BlockedTrackingPath(path) => {
                write!(f, "blocked tracking script/pixel path ({path})")
            }
        }
    }
}

/// Tracker, ad, and malicious-host filter.
///
/// `enabled` starts true. Custom blocked domains are added to the built-in
/// suffix list. Allowed domains are checked first and suppress every block
/// rule, including path rules. The block counter is caller-driven through
/// [`Self::check_and_count`] or [`Self::record_block`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkShield {
    /// When false, inspection allows every URL and does not consult the lists.
    pub enabled: bool,
    custom_blocked_domains: Vec<String>,
    allowed_domains: Vec<String>,
    blocked_total: u64,
}

impl Default for NetworkShield {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkShield {
    /// Shield enabled, with empty custom block and allow lists and a zero count.
    pub fn new() -> Self {
        Self {
            enabled: true,
            custom_blocked_domains: Vec::new(),
            allowed_domains: Vec::new(),
            blocked_total: 0,
        }
    }

    /// Number of times a block was recorded. Inspection alone does not increment this.
    pub fn blocked_total(&self) -> u64 {
        self.blocked_total
    }

    /// Increment the block counter. Saturates at `u64::MAX`.
    pub fn record_block(&mut self) {
        self.blocked_total = self.blocked_total.saturating_add(1);
    }

    /// Add a custom blocked domain suffix.
    ///
    /// Trimmed, lowercased, and stored without a leading dot. Empty input and
    /// duplicates are ignored. The domain is not checked for label shape.
    pub fn add_blocked_domain(&mut self, domain: &str) {
        let clean = domain.trim().trim_start_matches('.').to_ascii_lowercase();
        if !clean.is_empty() && !self.custom_blocked_domains.contains(&clean) {
            self.custom_blocked_domains.push(clean);
        }
    }

    /// Add a domain that bypasses every block rule.
    ///
    /// Trimmed, lowercased, and stored without a leading dot. A later host
    /// matches when it equals this name or ends with `.{name}`.
    pub fn add_allowed_domain(&mut self, domain: &str) {
        let clean = domain.trim().trim_start_matches('.').to_ascii_lowercase();
        if !clean.is_empty() && !self.allowed_domains.contains(&clean) {
            self.allowed_domains.push(clean);
        }
    }

    /// Return a block reason for `target_url`, or `None` when it may proceed.
    ///
    /// `initiator_url` is used only for the cross-site path test. This function
    /// does not connect. Order: disabled shield, parse failure, allow list,
    /// domain suffixes (built-in then custom), host prefixes, then path patterns.
    pub fn inspect_url(
        &self,
        target_url: &str,
        initiator_url: Option<&str>,
    ) -> Option<ShieldBlockReason> {
        // Disabled shield refuses nothing, including hosts on the built-in list.
        if !self.enabled {
            return None;
        }
        // Unparseable URLs are not blocked here. Callers that connect still
        // reject them in `parse_url` before `TcpStream::connect`.
        let Ok(parsed) = parse_url(target_url) else {
            return None;
        };
        let host = parsed.host.to_ascii_lowercase();

        // Allow list wins over suffixes, prefixes, and paths.
        for allow in &self.allowed_domains {
            if host == *allow || host.ends_with(&format!(".{allow}")) {
                return None;
            }
        }

        for dom in BLOCKED_DOMAIN_SUFFIXES
            .iter()
            .copied()
            .chain(self.custom_blocked_domains.iter().map(String::as_str))
        {
            if host == dom || host.ends_with(&format!(".{dom}")) {
                return Some(ShieldBlockReason::BlockedDomain(dom.to_string()));
            }
        }

        for prefix in BLOCKED_HOST_PREFIXES {
            if host.starts_with(prefix) {
                return Some(ShieldBlockReason::BlockedTrackerSubdomain(host));
            }
        }

        let path_low = parsed.path.to_ascii_lowercase();
        for pat in BLOCKED_PATH_PATTERNS {
            if path_low.contains(pat) {
                // Same-site, or a missing initiator, skips path rules that are
                // not named as a pixel, beacon, fingerprint, pagead, ad server,
                // or telemetry collector.
                let cross_site = initiator_url.is_some_and(|init| !is_same_site(init, target_url));
                if cross_site
                    || pat.contains("beacon")
                    || pat.contains("fingerprint")
                    || pat.contains("pixel")
                    || pat.contains("pagead")
                    || pat.contains("adserver")
                    || pat.contains("telemetry")
                {
                    return Some(ShieldBlockReason::BlockedTrackingPath((*pat).to_string()));
                }
            }
        }

        None
    }

    /// [`Self::inspect_url`], then increment the counter when a reason is returned.
    pub fn check_and_count(
        &mut self,
        target_url: &str,
        initiator_url: Option<&str>,
    ) -> Option<ShieldBlockReason> {
        let reason = self.inspect_url(target_url, initiator_url);
        if reason.is_some() {
            self.record_block();
        }
        reason
    }
}

/// [`NetworkShield::new`] followed by [`NetworkShield::inspect_url`].
///
/// Does not keep the block counter. Used by [`crate::fetch`] and
/// [`crate::fetch_post`] before they connect.
pub fn is_url_blocked_by_default_shield(
    target_url: &str,
    initiator_url: Option<&str>,
) -> Option<ShieldBlockReason> {
    NetworkShield::new().inspect_url(target_url, initiator_url)
}
