// Stored script text for anvil://downloads. frontend.rs copies this file into
// a script element after anvil_core.js.
// Author: Buntos

function scanDownloadItems() {
  let items = document.querySelectorAll('.history-item');
  writeAnvilSession('anvil.downloads.visible_items', String(items.length));
  return items.length;
}

function bindDownloadItems() {
  let items = document.querySelectorAll('.history-item');
  let index = 0;
  items.forEach((item) => {
    let slot = index;
    item.addEventListener('click', () => {
      writeAnvilSession('anvil.downloads.selected_item', String(slot));
    });
    index = index + 1;
  });
  return index;
}

function initAnvilDownloads() {
  recordAnvilPageVisit('downloads');
  scanDownloadItems();
  bindDownloadItems();
}

initAnvilDownloads();
