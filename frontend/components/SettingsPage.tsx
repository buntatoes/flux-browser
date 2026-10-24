// Stored text for flux://settings. frontend.rs
// settings_html_with_profile_state_and_engine compiles it.
// Only the markup block is kept. SETTINGS_JS is added later as a script
// element and is not part of this file.
// Props filled by the compiler: version, activeSearchEngineLabel,
// googleCardClass, ddgCardClass, bingCardClass, scriptsEnabledStatus,
// scriptsToggleLabel, scriptsToggleClass, jitEnabledStatus, jitToggleLabel,
// jitToggleClass, httpsOnlyStatus, httpsToggleLabel, httpsToggleClass,
// gpuBackendLabel, gpuVideoStatus, gpuToggleLabel, gpuToggleClass,
// framesDecoded, framesEncoded, profileStatus, bookmarkCount, historyCount,
// cookieCount.
// Lines above the markup block are not copied into the page.
// Author: Buntos

export interface SettingsPageProps {
  version: string; // Passed in. The markup block does not read it.
  activeSearchEngineLabel: string; // Name of the selected search engine.
  googleCardClass: string; // Class for the Google card.
  ddgCardClass: string; // Class for the DuckDuckGo card.
  bingCardClass: string; // Class for the Bing card.
  scriptsEnabledStatus: string; // Sentence for script execution.
  scriptsToggleLabel: string; // Text on the script toggle.
  scriptsToggleClass: string; // Class on the script toggle.
  jitEnabledStatus: string; // Sentence for the math helper.
  jitToggleLabel: string; // Text on the math-helper toggle.
  jitToggleClass: string; // Class on the math-helper toggle.
  httpsOnlyStatus: string; // Sentence for strict https.
  httpsToggleLabel: string; // Text on the https toggle.
  httpsToggleClass: string; // Class on the https toggle.
  gpuBackendLabel: string; // Video backend name.
  gpuVideoStatus: string; // On or off sentence for video.
  gpuToggleLabel: string; // Text on the video toggle.
  gpuToggleClass: string; // Class on the video toggle.
  framesDecoded: string; // Decoded frame count, already formatted.
  framesEncoded: string; // Encoded frame count, already formatted.
  profileStatus: string; // Profile seal sentence.
  bookmarkCount: string; // Bookmark count, already formatted.
  historyCount: string; // History count, already formatted.
  cookieCount: string; // Cookie count, already formatted.
}

// Markup below is extracted. Do not put page-visible notes in that block.

export function SettingsPage(props: SettingsPageProps) {
  return (
    <div className="settings-shell">
      <div className="brand-bar">
        <img id="flux-brand-logo" src="flux://logo.png" width="36" height="36" alt="Flux" />
        <p className="brand-title">Flux Settings</p>
        <p className="brand-nav">
          <a href="flux://home">Home</a> | <a href="flux://settings">Settings</a> | <a href="flux://downloads">Downloads</a> | <a href="flux://history">History</a> | <a href="flux://security">Security</a> | <a href="flux://inspect">Inspector</a>
        </p>
      </div>

      <div className="settings-layout">
        <div className="settings-sidebar">
          <p className="sidebar-heading">CATEGORIES</p>
          <div id="nav-sec-all" className="sidebar-item-active">
            <a href="flux://settings">All Settings</a>
          </div>
          <div id="nav-sec-search" className="sidebar-item">
            <a href="flux://settings?section=search">Search Engine</a>
          </div>
          <div id="nav-sec-js" className="sidebar-item">
            <a href="flux://settings?section=js">JavaScript</a>
          </div>
          <div id="nav-sec-privacy" className="sidebar-item">
            <a href="flux://settings?section=privacy">Privacy &amp; Security</a>
          </div>
          <div id="nav-sec-system" className="sidebar-item">
            <a href="flux://settings?section=system">System &amp; Hardware</a>
          </div>
        </div>

        <div className="settings-main">
          <div className="settings-card" data-section="search">
            <p className="sec-title">Default Search Engine</p>
            <p className="sec-desc">
              Active Engine: {props.activeSearchEngineLabel} — Used in the address bar and on the home screen search box.
            </p>
            <div className="engine-grid">
              <div className={props.googleCardClass}>
                <img src="flux://engine-google.png" width="32" height="32" alt="Google" />
                <div className="engine-card-text">
                  <p className="engine-name">Google Search</p>
                  <p className="engine-url">www.google.com</p>
                </div>
                <a className="setting-action-pill" href="flux://settings?engine=google">Use Google</a>
              </div>

              <div className={props.ddgCardClass}>
                <img src="flux://engine-ddg.png" width="32" height="32" alt="DuckDuckGo" />
                <div className="engine-card-text">
                  <p className="engine-name">DuckDuckGo</p>
                  <p className="engine-url">duckduckgo.com</p>
                </div>
                <a className="setting-action-pill" href="flux://settings?engine=duckduckgo">Use DuckDuckGo</a>
              </div>

              <div className={props.bingCardClass}>
                <img src="flux://engine-bing.png" width="32" height="32" alt="Microsoft Bing" />
                <div className="engine-card-text">
                  <p className="engine-name">Microsoft Bing</p>
                  <p className="engine-url">www.bing.com</p>
                </div>
                <a className="setting-action-pill" href="flux://settings?engine=bing">Use Bing</a>
              </div>
            </div>
          </div>

          <div className="settings-card" data-section="js">
            <p className="sec-title">JavaScript</p>
            <div className="setting-row">
              <div className="setting-info">
                <p className="setting-label">JavaScript Execution</p>
                <p className="sec-desc">
                  {props.scriptsEnabledStatus} — Allows websites to run interactive scripts and DOM updates inside the 100,000-step sandbox.
                </p>
              </div>
              <div className={props.scriptsToggleClass}>
                <a href="flux://settings?toggle=scripts">{props.scriptsToggleLabel}</a>
              </div>
            </div>

            <div className="setting-row">
              <div className="setting-info">
                <p className="setting-label">C++ math helper</p>
                <p className="sec-desc">
                  {props.jitEnabledStatus}
                </p>
              </div>
              <div className={props.jitToggleClass}>
                <a href="flux://settings?toggle=jit">{props.jitToggleLabel}</a>
              </div>
            </div>
          </div>

          <div className="settings-card" data-section="privacy">
            <p className="sec-title">Privacy, HTTPS Transport &amp; Encrypted Profile</p>
            <div className="setting-row">
              <div className="setting-info">
                <p className="setting-label">Strict HTTPS-Only Transport Mode</p>
                <p className="sec-desc">
                  {props.httpsOnlyStatus} — Upgrades or blocks cleartext HTTP traffic while enforcing TLS strong cryptography.
                </p>
              </div>
              <div className={props.httpsToggleClass}>
                <a href="flux://settings?toggle=https_only">{props.httpsToggleLabel}</a>
              </div>
            </div>

            <div className="setting-row">
              <div className="setting-info">
                <p className="setting-label">Encrypted Local Profile (FLXP Container)</p>
                <p className="sec-desc">
                  {props.profileStatus} | Bookmarks: {props.bookmarkCount} | History: {props.historyCount} | Cookies: {props.cookieCount}
                </p>
              </div>
              <div className="setting-actions-inline">
                <a className="setting-action-pill" href="flux://settings?action=save_profile">Save Snapshot</a>
                <a className="setting-action-pill" href="flux://settings?action=clear_profile">Clear Profile</a>
              </div>
            </div>
          </div>

          <div className="settings-card" data-section="system">
            <p className="sec-title">Video</p>
            <div className="setting-row">
              <div className="setting-info">
                <p className="setting-label">Video</p>
                <p className="sec-desc">
                  Backend: {props.gpuBackendLabel} | {props.gpuVideoStatus} | Decoded: {props.framesDecoded} | Encoded: {props.framesEncoded} | C++20 Raster Kernel: Active
                </p>
              </div>
              <div className={props.gpuToggleClass}>
                <a href="flux://settings?toggle=gpu_video">{props.gpuToggleLabel}</a>
              </div>
            </div>
            <video id="gpu-demo-video" src="flux://demo.mp4" width="240" height="110"></video>
          </div>
        </div>
      </div>
    </div>
  );
}
