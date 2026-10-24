// Stored text for root://downloads. frontend.rs downloads_html_with_entries
// compiles it. Only the markup block is kept. There is no script file.
// Props filled by the compiler: version, entryCount.
// The DOWNLOAD_ITEMS_SLOT marker in the markup is replaced with one block
// per download, or the empty-state sentence.
// Lines above the markup block are not copied into the page.
// Author: Buntos

export interface DownloadsPageProps {
  version: string; // Shown in the badge line.
  entryCount: string; // File count, already formatted.
}

// Markup below is extracted. Do not put page-visible notes in that block.

export function DownloadsPage(props: DownloadsPageProps) {
  return (
    <div className="wrap">
      <div className="brand-bar">
        <img id="root-brand-logo" src="root://logo.png" width="38" height="38" alt="Root" />
        <p className="brand-title">Root Downloads</p>
        <p className="brand-nav">
          <a href="root://home">Start</a> | <a href="root://inspect">Inspector</a> | <a href="root://settings">Settings</a> | <a href="root://downloads">Downloads</a> | <a href="root://history">History</a> | <a href="root://security">Security</a>
        </p>
      </div>
      <div className="hero">
        <p className="badge">ROOT BROWSER {props.version} — SHA-256 VERIFIER &amp; SAFE QUARANTINE</p>
        <h1>Download Manager ({props.entryCount} Files)</h1>
        <p className="subtitle">
          Every downloaded payload is verified with a FIPS 180-4 SHA-256 checksum, sanitized against path traversal, tagged with Mark-of-the-Web (ZoneId=3), and checked for executable quarantine.
        </p>
      </div>

      <div className="section">
        <p className="sec-title">Quick Verification &amp; Test Downloads</p>
        <p className="sec-desc">
          Generate and verify an in-browser diagnostic bundle or CENC-encrypted MP4 stream with live SHA-256 integrity hashing:
        </p>
        <p>
          <a href="root://downloads?action=sample_report">Download Verified Diagnostic Report (JSON)</a> | <a href="root://downloads?action=sample_mp4">Download CENC Encrypted Stream (MP4)</a> | <a href="root://downloads?action=clear">Clear Download Log</a>
        </p>
      </div>

      <div id="root-downloads-list" className="history-list">
        {{DOWNLOAD_ITEMS_SLOT}}
      </div>

      <p>
        <a href="root://home">Return to Root Start</a> | <a href="root://settings">Open Settings &amp; EME Video</a> | <a href="root://security">Open Security Center</a>
      </p>
    </div>
  );
}
