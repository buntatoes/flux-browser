// Shared front-end controller utilities for built-in anvil:// pages.
// Author: Buntos

function readAnvilPref(key, fallbackValue) {
  let stored = localStorage.getItem(key);
  if (stored === null || stored === undefined || stored === '') {
    return fallbackValue;
  }
  return stored;
}

function writeAnvilPref(key, value) {
  localStorage.setItem(key, String(value));
  return value;
}

function readAnvilSession(key, fallbackValue) {
  let stored = sessionStorage.getItem(key);
  if (stored === null || stored === undefined || stored === '') {
    return fallbackValue;
  }
  return stored;
}

function writeAnvilSession(key, value) {
  sessionStorage.setItem(key, String(value));
  return value;
}

function recordAnvilPageVisit(pageSlug) {
  let countKey = 'anvil.visits.' + pageSlug;
  let prevRaw = localStorage.getItem(countKey);
  let nextCount = 1;
  if (prevRaw !== null && prevRaw !== '') {
    let parsed = parseInt(prevRaw, 10);
    if (!isNaN(parsed) && parsed >= 0) {
      nextCount = parsed + 1;
    }
  }
  localStorage.setItem(countKey, String(nextCount));
  sessionStorage.setItem('anvil.last_page', pageSlug);
  return nextCount;
}

function parseAnvilQueryParam(hrefText, keyName) {
  let raw = String(hrefText);
  let qIdx = raw.indexOf('?');
  if (qIdx < 0) {
    return '';
  }
  let query = raw.substring(qIdx + 1);
  let parts = query.split('&');
  for (let i = 0; i < parts.length; i++) {
    let pair = parts[i];
    let eqIdx = pair.indexOf('=');
    if (eqIdx > 0) {
      let k = pair.substring(0, eqIdx);
      let v = pair.substring(eqIdx + 1);
      if (k === keyName) {
        return v;
      }
    }
  }
  return '';
}

function setElementDisplayById(elId, displayMode) {
  let el = document.getElementById(elId);
  if (el) {
    el.style.display = displayMode;
    return true;
  }
  return false;
}

function setElementTextById(elId, textValue) {
  let el = document.getElementById(elId);
  if (el) {
    el.textContent = String(textValue);
    return true;
  }
  return false;
}

function filterElementsByAttribute(selector, attrName, expectedValue) {
  let nodes = document.querySelectorAll(selector);
  let visibleCount = 0;
  nodes.forEach((node) => {
    let actual = node.getAttribute(attrName);
    if (expectedValue === 'all' || actual === expectedValue) {
      node.style.display = 'block';
      visibleCount = visibleCount + 1;
    } else {
      node.style.display = 'none';
    }
  });
  return visibleCount;
}

function highlightActiveNavItem(activeId, itemIds) {
  for (let i = 0; i < itemIds.length; i++) {
    let currentId = itemIds[i];
    let el = document.getElementById(currentId);
    if (el) {
      if (currentId === activeId) {
        el.setAttribute('class', 'sidebar-item-active');
      } else {
        el.setAttribute('class', 'sidebar-item');
      }
    }
  }
  return activeId;
}

function countSelectorMatches(selector) {
  let nodes = document.querySelectorAll(selector);
  return nodes.length;
}
