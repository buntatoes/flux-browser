// Stored script text for anvil://history. frontend.rs copies this file into
// a script element after anvil_core.js.
// Author: Buntos

function scanHistoryEntries() {
  let items = document.querySelectorAll('.history-item');
  writeAnvilSession('anvil.history.visible_items', String(items.length));
  return items.length;
}

function bindHistoryEntries() {
  let items = document.querySelectorAll('.history-item');
  let index = 0;
  items.forEach((item) => {
    let slot = index;
    item.addEventListener('click', () => {
      writeAnvilSession('anvil.history.selected_item', String(slot));
    });
    index = index + 1;
  });
  return index;
}

function initAnvilHistory() {
  recordAnvilPageVisit('history');
  scanHistoryEntries();
  bindHistoryEntries();
}

initAnvilHistory();
