// Stored text for anvil://home. frontend.rs home_html_with_engine compiles it.
// Only the markup block is kept. HOME_JS is added later as a script element
// and is not part of this file.
// Props filled by the compiler: version, defaultQuery, activeEngineName,
// activeEngineLogo, searchPlaceholder.
// Lines above the markup block are not copied into the page.
// Author: Buntos

export interface StartDashboardProps {
  version: string; // Passed in. The markup block does not read it.
  defaultQuery: string; // Value of the search field. Usually empty.
  activeEngineName: string; // Accessible name for the engine image.
  activeEngineLogo: string; // Image address for the engine badge.
  searchPlaceholder: string; // Placeholder on the search field.
}

// Markup below is extracted. Do not put page-visible notes in that block.

export function StartDashboard(props: StartDashboardProps) {
  return (
    <div className="home-blank">
      <div className="home-brand-row">
        <img id="anvil-brand-logo" src="anvil://logo.png" width="68" height="68" alt="Anvil" />
        <h1 className="home-title">Anvil</h1>
      </div>

      <div className="home-search-wrap">
        <form className="search-row" action="anvil://navigate" method="GET">
          <img
            id="home-engine-badge"
            src={props.activeEngineLogo}
            width="28"
            height="28"
            alt={props.activeEngineName}
          />
          <input
            id="home-q"
            type="text"
            name="q"
            value={props.defaultQuery}
            placeholder={props.searchPlaceholder}
          />
          <button id="home-go" type="submit">
            Search
          </button>
        </form>
      </div>
    </div>
  );
}
