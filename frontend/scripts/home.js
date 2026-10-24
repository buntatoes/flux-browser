// Stored script text for anvil://home. frontend.rs copies this file into a
// script element after anvil_core.js.
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
      writeAnvilSession('anvil.home.active_engine', altName);
    }
    engineBadge.addEventListener('click', () => {
      writeAnvilPref('anvil.settings.section', 'search');
      window.location.href = 'anvil://settings?section=search';
    });
    return true;
  }
  return false;
}

function bindHomeBrandLogo() {
  let brandLogo = document.getElementById('anvil-brand-logo');
  if (brandLogo) {
    brandLogo.addEventListener('click', () => {
      focusHomeSearchBox();
    });
    return true;
  }
  return false;
}

function initAnvilHome() {
  recordAnvilPageVisit('home');
  focusHomeSearchBox();
  bindHomeEngineBadge();
  bindHomeBrandLogo();
}

initAnvilHome();
