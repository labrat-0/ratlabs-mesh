// Small shared helpers: DOM lookup, escaping, and the JSON API.
export const $ = s => document.querySelector(s);
export const esc = s => String(s ?? '').replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
export const api = (p, o) => fetch(p, o).then(async r => {
  if (!r.ok) throw new Error((await r.json().catch(() => ({}))).detail || r.statusText);
  return r.json();
});
// Toggle `on` across a segmented control's buttons.
export const pick = (group, btn) => [...group.children].forEach(x => x.classList.toggle('on', x === btn));
