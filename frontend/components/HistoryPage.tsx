// Stored text for flux://history. frontend.rs history_html_with_entries
// compiles it. Only the markup block is kept. There is no script file.
// Props filled by the compiler: version, entryCount.
// The HISTORY_ITEMS_SLOT marker in the markup is replaced with one block
// per history entry, or the empty-state sentence.
// Lines above the markup block are not copied into the page.
// Author: Buntos

export interface HistoryPageProps {
  version: string; // Shown in the badge line.
  entryCount: string; // Entry count, already formatted.
}

// Markup below is extracted. Do not put page-visible notes in that block.

export function HistoryPage(props: HistoryPageProps) {
  return (
    <div className="wrap">
      <div className="brand-bar">
        <img id="flux-brand-logo" src="flux://logo.png" width="38" height="38" alt="Flux" />
        <p className="brand-title">Flux History</p>
        <p className="brand-nav">
          <a href="flux://home">Start</a> | <a href="flux://inspect">Inspector</a> | <a href="flux://settings">Settings</a> | <a href="flux://downloads">Downloads</a> | <a href="flux://history">History</a> | <a href="flux://security">Security</a>
        </p>
      </div>
      <div className="hero">
        <p className="badge">FLUX BROWSER {props.version} — SESSION HISTORY</p>
        <h1>Browsing History ({props.entryCount} Entries)</h1>
        <p className="subtitle">
          Local zero-telemetry navigation log across active and closed tabs.
        </p>
      </div>

      <div id="flux-history-list" className="history-list">
        {{HISTORY_ITEMS_SLOT}}
      </div>

      <p>
        <a href="flux://history?action=clear">Clear Browsing History</a> | <a href="flux://settings">Open Settings &amp; GPU Video</a> | <a href="flux://home">Return to Flux Start</a>
      </p>
    </div>
  );
}
