// Stored script text for flux://downloads. frontend.rs copies this file into
// a script element after flux_core.js.
// Author: Buntos

function scanDownloadItems() {
  let items = document.querySelectorAll('.history-item');
  writeFluxSession('flux.downloads.visible_items', String(items.length));
  return items.length;
}

function bindDownloadItems() {
  let items = document.querySelectorAll('.history-item');
  let index = 0;
  items.forEach((item) => {
    let slot = index;
    item.addEventListener('click', () => {
      writeFluxSession('flux.downloads.selected_item', String(slot));
    });
    index = index + 1;
  });
  return index;
}

function initFluxDownloads() {
  recordFluxPageVisit('downloads');
  scanDownloadItems();
  bindDownloadItems();
}

initFluxDownloads();
