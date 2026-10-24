// Stored script text for anvil://inspect. frontend.rs copies this file into
// a script element after anvil_core.js.
// Author: Buntos

function recordInspectorSnapshot() {
  let cardNodes = document.querySelectorAll('.card');
  let rowNodes = document.querySelectorAll('tr');
  writeAnvilSession('anvil.inspect.card_count', String(cardNodes.length));
  writeAnvilSession('anvil.inspect.row_count', String(rowNodes.length));
  return cardNodes.length;
}

function bindInspectorCards() {
  let cards = document.querySelectorAll('.card');
  let index = 0;
  cards.forEach((card) => {
    let slot = index;
    card.addEventListener('click', () => {
      writeAnvilSession('anvil.inspect.selected_card', String(slot));
    });
    index = index + 1;
  });
  return index;
}

function initAnvilInspector() {
  recordAnvilPageVisit('inspect');
  recordInspectorSnapshot();
  bindInspectorCards();
}

initAnvilInspector();
