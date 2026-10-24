// FluxVM Built-in JavaScript Standard Library Prelude (Core Collections, Numeric & String Utilities)
// Author: Buntos
// License: Apache-2.0

function fluxClampNumber(val, minVal, maxVal) {
  let n = Number(val);
  if (n < minVal) {
    return minVal;
  }
  if (n > maxVal) {
    return maxVal;
  }
  return n;
}

function fluxLerpNumber(startVal, endVal, tVal) {
  let t = fluxClampNumber(tVal, 0, 1);
  return startVal + (endVal - startVal) * t;
}

function fluxFormatBytes(bytesVal) {
  let b = Number(bytesVal);
  if (b < 1024) {
    return String(b) + ' B';
  }
  if (b < 1048576) {
    let kb = Math.round((b * 10) / 1024) / 10;
    return String(kb) + ' KiB';
  }
  let mb = Math.round((b * 10) / 1048576) / 10;
  return String(mb) + ' MiB';
}

function fluxArraySum(items) {
  let total = 0;
  let len = items.length;
  for (let i = 0; i < len; i++) {
    total += Number(items[i]);
  }
  return total;
}

function fluxArrayMin(items) {
  let len = items.length;
  if (len === 0) {
    return 0;
  }
  let best = Number(items[0]);
  for (let i = 1; i < len; i++) {
    let cur = Number(items[i]);
    if (cur < best) {
      best = cur;
    }
  }
  return best;
}

function fluxArrayMax(items) {
  let len = items.length;
  if (len === 0) {
    return 0;
  }
  let best = Number(items[0]);
  for (let i = 1; i < len; i++) {
    let cur = Number(items[i]);
    if (cur > best) {
      best = cur;
    }
  }
  return best;
}

function fluxStringPadStart(inputStr, targetLen, padChar) {
  let s = String(inputStr);
  let pad = String(padChar);
  if (pad === '') {
    pad = ' ';
  }
  while (s.length < targetLen) {
    s = pad + s;
  }
  return s;
}

function fluxStringPadEnd(inputStr, targetLen, padChar) {
  let s = String(inputStr);
  let pad = String(padChar);
  if (pad === '') {
    pad = ' ';
  }
  while (s.length < targetLen) {
    s = s + pad;
  }
  return s;
}

function fluxNormalizeQuery(rawQuery) {
  let q = String(rawQuery).trim();
  return q.toLowerCase();
}

function fluxHashStringDjb2(textVal) {
  let s = String(textVal);
  let acc = 5381;
  let n = s.length;
  for (let i = 0; i < n; i++) {
    acc = (acc * 33) + i + 1;
  }
  return acc;
}
