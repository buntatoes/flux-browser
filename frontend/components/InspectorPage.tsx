// Stored text for root://inspect. frontend.rs inspect_html_with_state
// compiles it. Only the markup block is kept. There is no script file.
// Props filled by the compiler: version, tabCount, activeZoom, gpuBackend,
// shieldBlocked, hstsCount, downloadCount, cookieCount.
// Lines above the markup block are not copied into the page.
// Author: Buntos

export interface InspectorPageProps {
  version: string; // Shown in the badge line.
  tabCount: string; // Open tab count, already formatted.
  activeZoom: string; // Zoom percent, already formatted with a percent sign.
  gpuBackend: string; // Video backend label.
  shieldBlocked: string; // Shield block count, already formatted.
  hstsCount: string; // HSTS host count, already formatted.
  downloadCount: string; // Download count, already formatted.
  cookieCount: string; // Cookie count, already formatted.
}

// Markup below is extracted. Do not put page-visible notes in that block.

export function InspectorPage(props: InspectorPageProps) {
  return (
    <div className="wrap">
      <div className="brand-bar">
        <img id="root-brand-logo" src="root://logo.png" width="38" height="38" alt="Root" />
        <p className="brand-title">Root Developer Inspector</p>
        <p className="brand-nav">
          <a href="root://home">Start</a> | <a href="root://inspect">Inspector</a> | <a href="root://settings">Settings</a> | <a href="root://downloads">Downloads</a> | <a href="root://history">History</a> | <a href="root://security">Security</a>
        </p>
      </div>

      <div className="hero">
        <p className="badge">
          PROOF OF CONCEPT — DEVTOOLS &amp; ENGINE DIAGNOSTICS ({props.version}) — MOUSE DOCK (F12) &amp; FULL VIEW
        </p>
        <h1>Developer Inspector &amp; Engine Telemetry</h1>
        <p className="sub">
          Inspect live DOM arena budgets, CSS cascade specificity, DisplayList SDF paint commands, HSTS/SRI/CSP transport policies, and W3C EME/NV12 media pipelines. Click the [&lt;&gt;] Inspector button in the top toolbar or press F12 on any page to toggle the live bottom Inspector Dock.
        </p>
        <p>
          <a href="examples/hello.html">Inspect Local Demo DOM (examples/hello.html)</a> | <a href="root://settings">Inspect GPU Video &amp; EME Stream</a> | <a href="root://downloads?action=sample">Run SHA-256 Download Probe</a>
        </p>
      </div>

      <div className="cards">
        <div className="card">
          <p className="card-title">1. DOM &amp; Layout Engine</p>
          <p className="card-body">
            Arena-indexed Document tree (MAX_DOM_NODES: 50,000, MAX_DOM_DEPTH: 256) with Flexbox, HTML Table, and out-of-flow layout.
          </p>
          <p className="badge">Active Tabs: {props.tabCount} | Viewport Zoom: {props.activeZoom}</p>
        </div>

        <div className="card">
          <p className="card-title">2. Network, HSTS &amp; SRI</p>
          <p className="card-body">
            Pre-socket tracker/malware shield, RFC 6797 HSTS preload/dynamic store, and FIPS 180-4 SHA-256 Subresource Integrity.
          </p>
          <p className="badge">Blocked: {props.shieldBlocked} | HSTS Hosts: {props.hstsCount}</p>
        </div>

        <div className="card">
          <p className="card-title">3. GPU, EME &amp; Storage</p>
          <p className="card-body">
            NV12 BT.709 hardware video, W3C ClearKey / CENC MP4 decryptor, and Windows DPAPI sealed profile container.
          </p>
          <p className="badge">GPU: {props.gpuBackend} | Downloads: {props.downloadCount}</p>
        </div>
      </div>

      <div className="section">
        <p className="sec-title">Engine Subsystem &amp; Security Budget Matrix</p>
        <table id="root-inspector-table">
          <tr>
            <th>Subsystem</th>
            <th>Active Implementation</th>
            <th>Hardened Ceiling / Invariant</th>
            <th>Status</th>
          </tr>
          <tr>
            <td>DOM Arena (root-dom)</td>
            <td>NodeId Arena + Implicit Table Auto-Close</td>
            <td>50,000 Nodes / 256 Depth / 128 Attrs</td>
            <td>ACTIVE</td>
          </tr>
          <tr>
            <td>CSS &amp; Layout (root-style / root-layout)</td>
            <td>Specificity Cascade + @media + Flexbox + Table</td>
            <td>TrueType Metrics + Z-Index Clip Stack</td>
            <td>ACTIVE</td>
          </tr>
          <tr>
            <td>Raster &amp; Compositor (root-raster / root-gpu)</td>
            <td>SDF Rounded Quads, Gradients, Shadows &amp; NV12</td>
            <td>16 MP (64 MiB) Image Allocation Cap</td>
            <td>ACTIVE</td>
          </tr>
          <tr>
            <td>Script Runtime (root-script)</td>
            <td>Deterministic Interpreter + W3C EME Access</td>
            <td>100,000 Steps / 64 Call Depth / SOP Fetch</td>
            <td>ACTIVE</td>
          </tr>
          <tr>
            <td>Transport &amp; Shield (root-net)</td>
            <td>Schannel / OpenSSL TLS + HSTS + SRI SHA-256</td>
            <td>8 MiB Raw / 16 MiB Decompressed Cap</td>
            <td>ACTIVE</td>
          </tr>
          <tr>
            <td>Profile &amp; Vault (profile.rs / downloads.rs)</td>
            <td>Windows DPAPI (crypt32.dll) + SHA-256 MOTW</td>
            <td>Cookies: {props.cookieCount} | Downloads: {props.downloadCount}</td>
            <td>ACTIVE</td>
          </tr>
        </table>
      </div>
    </div>
  );
}
