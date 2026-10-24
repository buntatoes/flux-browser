// Stored text for flux://security. frontend.rs security_html compiles it.
// Only the markup block is kept. There is no script file.
// Prop filled by the compiler: version.
// Lines above the markup block are not copied into the page.
// Author: Buntos

export interface SecurityCenterProps {
  version: string; // Shown in the heading.
}

// Markup below is extracted. Do not put page-visible notes in that block.

export function SecurityCenter(props: SecurityCenterProps) {
  return (
    <div className="wrap">
      <div className="brand-bar">
        <img id="flux-brand-logo" src="flux://logo.png" width="38" height="38" alt="Flux" />
        <p className="brand-title">Flux Security Center</p>
        <p className="brand-nav">
          <a href="flux://home">Start</a> | <a href="flux://inspect">Inspector</a> | <a href="flux://settings">Settings</a> | <a href="flux://downloads">Downloads</a> | <a href="flux://history">History</a> | <a href="flux://security">Security</a>
        </p>
      </div>
      <div className="banner">
        <h1>Security ({props.version})</h1>
        <p className="sub">
          Linux uses OpenSSL for HTTPS. The profile file is AES-256-GCM. Windows uses Schannel and DPAPI. Pages are drawn in this process. A panic in a background load is caught.
        </p>
      </div>

      <div className="section">
        <p className="sec-title">1. Process limits</p>
        <p className="sec-desc">
          On Linux the window sets no_new_privs, turns core dumps off, and tries a 2 GiB address-space cap. The tab worker, when launched, uses 256 MiB.
        </p>
      </div>

      <div className="section">
        <p className="sec-title">2. Profile file</p>
        <p className="sec-desc">
          Windows seals the profile with DPAPI. Linux seals it with AES-256-GCM. The key is profile.key in the same directory.
        </p>
      </div>

      <div className="section">
        <p className="sec-title">3. Strict-Transport-Security (HSTS) &amp; Subresource Integrity (SRI SHA-256)</p>
        <p className="sec-desc">
          Automatically upgrades HSTS hosts from HTTP to HTTPS and verifies FIPS 180-4 SHA-256 integrity hashes on external scripts and stylesheets before execution.
        </p>
      </div>

      <div className="section">
        <p className="sec-title">4. Pre-Socket Tracker, Ad-Tech, &amp; Malicious Domain Shield</p>
        <p className="sec-desc">
          Blocks ad networks, behavioral telemetry/beacon endpoints, browser fingerprinting scripts, and known phishing/cryptomining/malware hosts before any connection is opened.
        </p>
      </div>

      <div className="section">
        <p className="sec-title">5. Content Security Policy (CSP), CORP, &amp; Cookie Protection</p>
        <p className="sec-desc">
          Enforces HTTP and meta Content-Security-Policy directives, Cross-Origin-Resource-Policy headers, and strict cookie scoping (HttpOnly, Secure, SameSite).
        </p>
      </div>

      <p>
        <a href="flux://home">Return to Flux Start</a>
      </p>
    </div>
  );
}
