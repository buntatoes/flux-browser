// Stored script text for root://security. frontend.rs copies this file into
// a script element after root_core.js.
// Author: Buntos

function scanSecuritySections() {
  let sections = document.querySelectorAll('.section');
  writeRootSession('root.security.section_count', String(sections.length));
  return sections.length;
}

function bindSecuritySections() {
  let sections = document.querySelectorAll('.section');
  let index = 0;
  sections.forEach((sec) => {
    let sectionIndex = index;
    sec.addEventListener('click', () => {
      writeRootSession('root.security.active_section', String(sectionIndex));
    });
    index = index + 1;
  });
  return index;
}

function initRootSecurity() {
  recordRootPageVisit('security');
  scanSecuritySections();
  bindSecuritySections();
}

initRootSecurity();
