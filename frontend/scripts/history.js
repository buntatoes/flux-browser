// Stored script text for flux://history. frontend.rs copies this file into
// a script element after flux_core.js.
// Author: Buntos

function scanHistoryEntries() {
  let items = document.querySelectorAll('.history-item');
  writeFluxSession('flux.history.visible_items', String(items.length));
  return items.length;
}

function bindHistoryEntries() {
  let items = document.querySelectorAll('.history-item');
  let index = 0;
  items.forEach((item) => {
    let slot = index;
    item.addEventListener('click', () => {
      writeFluxSession('flux.history.selected_item', String(slot));
    });
    index = index + 1;
  });
  return index;
}

function initFluxHistory() {
  recordFluxPageVisit('history');
  scanHistoryEntries();
  bindHistoryEntries();
}

initFluxHistory();
