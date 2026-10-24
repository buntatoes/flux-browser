// Stored text. frontend.rs compile_tsx_component keeps the markup block and
// replaces className with class. This file is the window frame document, not
// a root:// page. render_react_shell_html fills the props below and replaces
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

export function RootBrowserApp(props: BrowserShellProps) {
  return (
    <div id="root-react-shell" className="root-shell">
      <div id="root-tab-strip" className="tab-strip">
        <img
          id="root-shell-logo"
          src="root://logo.png"
          width="24"
          height="24"
          alt="Root"
          data-ui-action="home"
        />
        {{TABS_SLOT}}
        <button id="root-new-tab" className="new-tab-btn" data-ui-action="new-tab">
          +
        </button>
      </div>

      <div id="root-nav-bar" className="nav-bar">
        <button id="root-btn-back" className="nav-btn" data-ui-action="back">
          &lt;
        </button>
        <button id="root-btn-forward" className="nav-btn" data-ui-action="forward">
          &gt;
        </button>
        <button id="root-btn-reload" className="nav-btn" data-ui-action="reload">
          R
        </button>
        <button id="root-btn-home" className="nav-btn" data-ui-action="home">
          H
        </button>

        <div
          id="root-security-badge"
          className={props.securityClass}
          data-ui-action="security-badge"
        >
          {props.securityLabel}
        </div>

        <div
          id="root-address-bar"
          className={props.addressClass}
          data-ui-action="address-field"
        >
          {props.addressDisplay}
        </div>

        <button
          id="root-btn-bookmark"
          className={props.starClass}
          data-ui-action="toggle-bookmark"
        >
          *
        </button>

        <button
          id="root-zoom-pill"
          className="zoom-pill"
          data-ui-action="zoom-pill"
        >
          {props.zoomDisplay}
        </button>

        <button
          id="root-btn-settings"
          className="settings-btn"
          data-ui-action="settings-button"
        >
          S
        </button>

        <button
          id="root-btn-downloads"
          className="nav-btn"
          data-ui-action="downloads-button"
        >
          DL
        </button>

        <button
          id="root-btn-inspector"
          className={props.inspectorClass}
          data-ui-action="inspector-button"
        >
          DEV
        </button>

        <button
          id="root-btn-app-menu"
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

      <div id="root-viewport-slot" className="viewport-slot">
        {{VIEWPORT_SLOT}}
      </div>

      {{INSPECTOR_DOCK_SLOT}}

      <div id="root-status-bar" className="status-bar">
        <span id="root-status-left" className="status-left">
          {props.statusLeft}
        </span>
        <span id="root-status-right" className="status-right">
          {props.statusRight}
        </span>
      </div>
    </div>
  );
}
