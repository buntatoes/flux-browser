// Stored script text for flux://inspect. frontend.rs copies this file into
// a script element after flux_core.js.
// Author: Buntos

function recordInspectorSnapshot() {
  let cardNodes = document.querySelectorAll('.card');
  let rowNodes = document.querySelectorAll('tr');
  writeFluxSession('flux.inspect.card_count', String(cardNodes.length));
  writeFluxSession('flux.inspect.row_count', String(rowNodes.length));
  return cardNodes.length;
}

function bindInspectorCards() {
  let cards = document.querySelectorAll('.card');
  let index = 0;
  cards.forEach((card) => {
    let slot = index;
    card.addEventListener('click', () => {
      writeFluxSession('flux.inspect.selected_card', String(slot));
    });
    index = index + 1;
  });
  return index;
}

function initFluxInspector() {
  recordFluxPageVisit('inspect');
  recordInspectorSnapshot();
  bindInspectorCards();
}

initFluxInspector();
