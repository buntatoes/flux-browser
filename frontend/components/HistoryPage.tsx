// Stored text for anvil://history. frontend.rs history_html_with_entries
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
        <img id="anvil-brand-logo" src="anvil://logo.png" width="38" height="38" alt="Anvil" />
        <p className="brand-title">Anvil History</p>
        <p className="brand-nav">
          <a href="anvil://home">Start</a> | <a href="anvil://inspect">Inspector</a> | <a href="anvil://settings">Settings</a> | <a href="anvil://downloads">Downloads</a> | <a href="anvil://history">History</a> | <a href="anvil://security">Security</a>
        </p>
      </div>
      <div className="hero">
        <p className="badge">ANVIL BROWSER {props.version} — SESSION HISTORY</p>
        <h1>Browsing History ({props.entryCount} Entries)</h1>
        <p className="subtitle">
          Local zero-telemetry navigation log across active and closed tabs.
        </p>
      </div>

      <div id="anvil-history-list" className="history-list">
        {{HISTORY_ITEMS_SLOT}}
      </div>

      <p>
        <a href="anvil://history?action=clear">Clear Browsing History</a> | <a href="anvil://settings">Open Settings &amp; GPU Video</a> | <a href="anvil://home">Return to Anvil Start</a>
      </p>
    </div>
  );
}
