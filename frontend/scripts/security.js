// Stored script text for anvil://security. frontend.rs copies this file into
// a script element after anvil_core.js.
// Author: Buntos

function scanSecuritySections() {
  let sections = document.querySelectorAll('.section');
  writeAnvilSession('anvil.security.section_count', String(sections.length));
  return sections.length;
}

function bindSecuritySections() {
  let sections = document.querySelectorAll('.section');
  let index = 0;
  sections.forEach((sec) => {
    let sectionIndex = index;
    sec.addEventListener('click', () => {
      writeAnvilSession('anvil.security.active_section', String(sectionIndex));
    });
    index = index + 1;
  });
  return index;
}

function initAnvilSecurity() {
  recordAnvilPageVisit('security');
  scanSecuritySections();
  bindSecuritySections();
}

initAnvilSecurity();
