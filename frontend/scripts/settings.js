// Stored script text for flux://settings. frontend.rs copies this file into
// a script element after flux_core.js.
// Author: Buntos

function switchSettingsSection(sectionName) {
  let cards = document.querySelectorAll('.settings-card');
  cards.forEach((card) => {
    let group = card.getAttribute('data-section');
    if (sectionName === 'all' || group === sectionName) {
      card.style.display = 'block';
    } else {
      card.style.display = 'none';
    }
  });
  writeFluxPref('flux.settings.section', sectionName);
  let navIds = [
    'nav-sec-all',
    'nav-sec-search',
    'nav-sec-js',
    'nav-sec-privacy',
    'nav-sec-system'
  ];
  let targetNavId = 'nav-sec-all';
  if (sectionName === 'search') {
    targetNavId = 'nav-sec-search';
  } else if (sectionName === 'js') {
    targetNavId = 'nav-sec-js';
  } else if (sectionName === 'privacy') {
    targetNavId = 'nav-sec-privacy';
  } else if (sectionName === 'system') {
    targetNavId = 'nav-sec-system';
  }
  highlightActiveNavItem(targetNavId, navIds);
}

function restoreInitialSettingsSection() {
  let href = window.location.href;
  let querySec = parseFluxQueryParam(href, 'section');
  if (querySec !== '') {
    switchSettingsSection(querySec);
    return querySec;
  }
  return 'all';
}

function initFluxSettings() {
  recordFluxPageVisit('settings');

  let navAll = document.getElementById('nav-sec-all');
  let navSearch = document.getElementById('nav-sec-search');
  let navPrivacy = document.getElementById('nav-sec-privacy');
  let navJs = document.getElementById('nav-sec-js');
  let navSystem = document.getElementById('nav-sec-system');

  if (navAll) {
    navAll.addEventListener('click', () => {
      switchSettingsSection('all');
    });
  }
  if (navSearch) {
    navSearch.addEventListener('click', () => {
      switchSettingsSection('search');
    });
  }
  if (navPrivacy) {
    navPrivacy.addEventListener('click', () => {
      switchSettingsSection('privacy');
    });
  }
  if (navJs) {
    navJs.addEventListener('click', () => {
      switchSettingsSection('js');
    });
  }
  if (navSystem) {
    navSystem.addEventListener('click', () => {
      switchSettingsSection('system');
    });
  }

  restoreInitialSettingsSection();
}

initFluxSettings();
