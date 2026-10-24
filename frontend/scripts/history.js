// Stored script text for root://history. frontend.rs copies this file into
// a script element after root_core.js.
// Author: Buntos

function scanHistoryEntries() {
  let items = document.querySelectorAll('.history-item');
  writeRootSession('root.history.visible_items', String(items.length));
  return items.length;
}

function bindHistoryEntries() {
  let items = document.querySelectorAll('.history-item');
  let index = 0;
  items.forEach((item) => {
    let slot = index;
    item.addEventListener('click', () => {
      writeRootSession('root.history.selected_item', String(slot));
    });
    index = index + 1;
  });
  return index;
}

function initRootHistory() {
  recordRootPageVisit('history');
  scanHistoryEntries();
  bindHistoryEntries();
}

initRootHistory();
