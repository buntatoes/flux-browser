// Stored script text for flux://home. frontend.rs copies this file into a
// script element after flux_core.js.
// Author: Buntos

function focusHomeSearchBox() {
  let searchInput = document.getElementById('home-q');
  if (searchInput) {
    searchInput.focus();
    return true;
  }
  return false;
}

function bindHomeEngineBadge() {
  let engineBadge = document.getElementById('home-engine-badge');
  if (engineBadge) {
    let altName = engineBadge.getAttribute('alt');
    if (altName !== null && altName !== '') {
      writeFluxSession('flux.home.active_engine', altName);
    }
    engineBadge.addEventListener('click', () => {
      writeFluxPref('flux.settings.section', 'search');
      window.location.href = 'flux://settings?section=search';
    });
    return true;
  }
  return false;
}

function bindHomeBrandLogo() {
  let brandLogo = document.getElementById('flux-brand-logo');
  if (brandLogo) {
    brandLogo.addEventListener('click', () => {
      focusHomeSearchBox();
    });
    return true;
  }
  return false;
}

function initFluxHome() {
  recordFluxPageVisit('home');
  focusHomeSearchBox();
  bindHomeEngineBadge();
  bindHomeBrandLogo();
}

initFluxHome();
