// Stored script text for root://inspect. frontend.rs copies this file into
// a script element after root_core.js.
// Author: Buntos

function recordInspectorSnapshot() {
  let cardNodes = document.querySelectorAll('.card');
  let rowNodes = document.querySelectorAll('tr');
  writeRootSession('root.inspect.card_count', String(cardNodes.length));
  writeRootSession('root.inspect.row_count', String(rowNodes.length));
  return cardNodes.length;
}

function bindInspectorCards() {
  let cards = document.querySelectorAll('.card');
  let index = 0;
  cards.forEach((card) => {
    let slot = index;
    card.addEventListener('click', () => {
      writeRootSession('root.inspect.selected_card', String(slot));
    });
    index = index + 1;
  });
  return index;
}

function initRootInspector() {
  recordRootPageVisit('inspect');
  recordInspectorSnapshot();
  bindInspectorCards();
}

initRootInspector();
