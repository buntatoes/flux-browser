// Stored text. frontend.rs compile_tsx_component keeps the markup block and
// replaces className with class. This file is the window frame document, not
// a anvil:// page. render_react_shell_html fills the props below and replaces
// these slot markers in the markup: TABS_SLOT, BOOKMARKS_SLOT, FIND_BAR_SLOT,
// APP_MENU_SLOT, CONTEXT_MENU_SLOT, VIEWPORT_SLOT, INSPECTOR_DOCK_SLOT.
// Props passed in: version, activeUrl, addressDisplay, addressClass,
// securityLabel, securityClass, zoomDisplay, starClass, inspectorClass,
// appMenuClass, statusLeft, statusRight.
// Lines above the markup block are not copied into the page.
// Author: Buntos

export interface TabItemProps {
  index: number; // Declared only. The tab strip is the TABS_SLOT marker.
  title: string;
  activeClass: string;
  closeVisible: boolean;
}

export interface BookmarkItemProps {
  index: number; // Declared only. Bookmarks are the BOOKMARKS_SLOT marker.
  title: string;
  url: string;
}

export interface BrowserShellProps {
  version: string; // Passed in. The markup block does not read it.
  activeUrl: string; // Passed in. The markup block does not read it.
  addressDisplay: string; // Address text, including the editing caret when focused.
  addressClass: string; // Class on the address field.
  securityLabel: string; // TLS, SYS, HTTP, or FILE.
  securityClass: string; // Class on the security badge.
  zoomDisplay: string; // Percent from the session, already formatted.
  starClass: string; // Class on the bookmark button.
  inspectorClass: string; // Class on the inspector button.
  appMenuClass: string; // Class on the app-menu button.
  statusLeft: string; // Left side of the status bar.
  statusRight: string; // Right side of the status bar.
  canGoBack: boolean; // Declared only. The markup block does not read it.
  canGoForward: boolean; // Declared only. The markup block does not read it.
}

// Markup below is extracted. Do not put page-visible notes in that block.

export function AnvilBrowserApp(props: BrowserShellProps) {
  return (
    <div id="anvil-react-shell" className="anvil-shell">
      <div id="anvil-tab-strip" className="tab-strip">
        <img
          id="anvil-shell-logo"
          src="anvil://logo.png"
          width="24"
          height="24"
          alt="Anvil"
          data-ui-action="home"
        />
        {{TABS_SLOT}}
        <button id="anvil-new-tab" className="new-tab-btn" data-ui-action="new-tab">
          +
        </button>
      </div>

      <div id="anvil-nav-bar" className="nav-bar">
        <button id="anvil-btn-back" className="nav-btn" data-ui-action="back">
          &lt;
        </button>
        <button id="anvil-btn-forward" className="nav-btn" data-ui-action="forward">
          &gt;
        </button>
        <button id="anvil-btn-reload" className="nav-btn" data-ui-action="reload">
          R
        </button>
        <button id="anvil-btn-home" className="nav-btn" data-ui-action="home">
          H
        </button>

        <div
          id="anvil-security-badge"
          className={props.securityClass}
          data-ui-action="security-badge"
        >
          {props.securityLabel}
        </div>

        <div
          id="anvil-address-bar"
          className={props.addressClass}
          data-ui-action="address-field"
        >
          {props.addressDisplay}
        </div>

        <button
          id="anvil-btn-bookmark"
          className={props.starClass}
          data-ui-action="toggle-bookmark"
        >
          *
        </button>

        <button
          id="anvil-zoom-pill"
          className="zoom-pill"
          data-ui-action="zoom-pill"
        >
          {props.zoomDisplay}
        </button>

        <button
          id="anvil-btn-settings"
          className="settings-btn"
          data-ui-action="settings-button"
        >
          S
        </button>

        <button
          id="anvil-btn-downloads"
          className="nav-btn"
          data-ui-action="downloads-button"
        >
          DL
        </button>

        <button
          id="anvil-btn-inspector"
          className={props.inspectorClass}
          data-ui-action="inspector-button"
        >
          DEV
        </button>

        <button
          id="anvil-btn-app-menu"
          className={props.appMenuClass}
          data-ui-action="app-menu-button"
        >
          =
        </button>
      </div>

      {{BOOKMARKS_SLOT}}

      {{FIND_BAR_SLOT}}

      {{APP_MENU_SLOT}}

      {{CONTEXT_MENU_SLOT}}

      <div id="anvil-viewport-slot" className="viewport-slot">
        {{VIEWPORT_SLOT}}
      </div>

      {{INSPECTOR_DOCK_SLOT}}

      <div id="anvil-status-bar" className="status-bar">
        <span id="anvil-status-left" className="status-left">
          {props.statusLeft}
        </span>
        <span id="anvil-status-right" className="status-right">
          {props.statusRight}
        </span>
      </div>
    </div>
  );
}
