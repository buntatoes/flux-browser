// Stored script text for root://home. frontend.rs copies this file into a
// script element after root_core.js.
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
      writeRootSession('root.home.active_engine', altName);
    }
    engineBadge.addEventListener('click', () => {
      writeRootPref('root.settings.section', 'search');
      window.location.href = 'root://settings?section=search';
    });
    return true;
  }
  return false;
}

function bindHomeBrandLogo() {
  let brandLogo = document.getElementById('root-brand-logo');
  if (brandLogo) {
    brandLogo.addEventListener('click', () => {
      focusHomeSearchBox();
    });
    return true;
  }
  return false;
}

function initRootHome() {
  recordRootPageVisit('home');
  focusHomeSearchBox();
  bindHomeEngineBadge();
  bindHomeBrandLogo();
}

initRootHome();
