// Stored script text for root://downloads. frontend.rs copies this file into
// a script element after root_core.js.
// Author: Buntos

function scanDownloadItems() {
  let items = document.querySelectorAll('.history-item');
  writeRootSession('root.downloads.visible_items', String(items.length));
  return items.length;
}

function bindDownloadItems() {
  let items = document.querySelectorAll('.history-item');
  let index = 0;
  items.forEach((item) => {
    let slot = index;
    item.addEventListener('click', () => {
      writeRootSession('root.downloads.selected_item', String(slot));
    });
    index = index + 1;
  });
  return index;
}

function initRootDownloads() {
  recordRootPageVisit('downloads');
  scanDownloadItems();
  bindDownloadItems();
}

initRootDownloads();
