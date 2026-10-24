// Stored script text for flux://security. frontend.rs copies this file into
// a script element after flux_core.js.
// Author: Buntos

function scanSecuritySections() {
  let sections = document.querySelectorAll('.section');
  writeFluxSession('flux.security.section_count', String(sections.length));
  return sections.length;
}

function bindSecuritySections() {
  let sections = document.querySelectorAll('.section');
  let index = 0;
  sections.forEach((sec) => {
    let sectionIndex = index;
    sec.addEventListener('click', () => {
      writeFluxSession('flux.security.active_section', String(sectionIndex));
    });
    index = index + 1;
  });
  return index;
}

function initFluxSecurity() {
  recordFluxPageVisit('security');
  scanSecuritySections();
  bindSecuritySections();
}

initFluxSecurity();
