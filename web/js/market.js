// Market: one search across Poly Haven, ambientCG, Sketchfab and Poly Pizza; downloads keep their licence.
import { $, api, esc, pick } from './api.js';
import { openJob, refreshLibrary } from './job.js';

export const mkt = { loaded: false, sources: {}, source: 'polyhaven', kind: 'model', page: 0, items: [] };
const KIND_LABEL = { model: 'models', material: 'materials', hdri: 'HDRI skies' };
let showTab = () => {};

export async function marketInit(tabs) {
  if (tabs) showTab = tabs;
  if (mkt.loaded) return;
  mkt.loaded = true;
  mkt.sources = await api('/api/market/sources');
  renderSources(); marketSearch(); refreshMarketLib();
}

export async function reloadSources() {
  if (!mkt.loaded) return;
  mkt.sources = await api('/api/market/sources'); renderSources();
}

function renderSources() {
  $('#mSources').innerHTML = Object.entries(mkt.sources).map(([k, s]) => {
    const fits = s.kinds.includes(mkt.kind);
    const lock = !s.ready ? (s.key_for === 'download' ? ' (key to download)' : ' (needs key)') : '';
    return `<button data-src="${k}" class="${k === mkt.source ? 'on' : ''}" ${fits ? '' : 'disabled'} title="${esc(s.note)}">${esc(s.label)}<small>${lock}</small></button>`;
  }).join('');
  const s = mkt.sources[mkt.source];
  $('#mNote').textContent = `${s.label}: ${s.note} Licence: ${s.licence}.` + (!s.ready ? ` Add ${s.key} under Keys (free account).` : '');
}
$('#mSources').addEventListener('click', e => { const b = e.target.closest('button'); if (!b || b.disabled) return; mkt.source = b.dataset.src; renderSources(); marketSearch(); });
$('#mKinds').addEventListener('click', e => {
  const b = e.target.closest('button'); if (!b) return;
  mkt.kind = b.dataset.kind; pick($('#mKinds'), b);
  if (!mkt.sources[mkt.source].kinds.includes(mkt.kind)) mkt.source = Object.keys(mkt.sources).find(k => mkt.sources[k].kinds.includes(mkt.kind));
  renderSources(); marketSearch();
});
$('#mForm').onsubmit = e => { e.preventDefault(); marketSearch(); };
$('#mMore').onclick = () => marketSearch(true);

async function marketSearch(more = false) {
  mkt.page = more ? mkt.page + 1 : 0;
  const src = mkt.sources[mkt.source];
  if (src.key_for === 'search' && !src.ready) { $('#mResults').innerHTML = `<p class="hint">${esc(src.label)} needs a free API key to search. Add ${src.key} under Keys.</p>`; $('#mMore').hidden = true; return; }
  $('#mHead').textContent = 'Searching...';
  try {
    const q = $('#mQ').value, r = await api(`/api/market/search?source=${mkt.source}&kind=${mkt.kind}&q=${encodeURIComponent(q)}&page=${mkt.page}`);
    mkt.items = more ? mkt.items.concat(r.items) : r.items;
    $('#mHead').textContent = `${src.label} · ${KIND_LABEL[mkt.kind]}${q ? ` · "${q}"` : ' · popular'} · ${r.total} found`;
    renderCards(); $('#mMore').hidden = mkt.items.length >= r.total || !r.items.length;
  } catch (err) { $('#mHead').textContent = 'Search failed'; $('#mResults').innerHTML = `<p class="warn">${esc(err.message)}</p>`; }
}

function renderCards() {
  $('#mResults').innerHTML = mkt.items.map((it, i) => {
    const by = /attribution|by/i.test(it.licence) && !/cc0/i.test(it.licence);
    const extra = [it.polycount ? `${Number(it.polycount).toLocaleString()} tris` : '', it.animated ? 'animated' : ''].filter(Boolean).join(' · ');
    return `<div class="card"><a class="thumb" href="${esc(it.page)}" target="_blank" rel="noopener" style="background-image:url('${esc(it.thumb)}')" title="Open on ${esc(mkt.sources[it.source].label)}"></a>
      <div class="meta"><span class="name" title="${esc(it.name)}">${esc(it.name)}</span>
      <span class="sub"><span class="lic ${by ? 'by' : ''}" title="${by ? 'Credit the author when you use it' : 'No credit needed'}">${esc(it.licence)}</span><span>${esc(extra)}</span></span>
      <span class="sub"><span>${esc(it.author || '')}</span></span></div>
      <div class="acts"><button class="${it.have ? 'ghost' : 'primary'} sm" data-grab="${i}">${it.have ? 'Have it' : 'Grab'}</button></div></div>`;
  }).join('') || '<p class="hint">Nothing found. Try another word or source.</p>';
}
$('#mResults').addEventListener('click', async e => {
  const b = e.target.closest('[data-grab]'); if (!b) return;
  const it = mkt.items[+b.dataset.grab]; b.disabled = true; b.textContent = 'Downloading...';
  const fd = new FormData(); fd.append('item', JSON.stringify(it)); fd.append('res', $('#mRes').value);
  try { await api('/api/market/grab', { method: 'POST', body: fd }); it.have = true; b.textContent = 'Have it'; b.className = 'ghost sm'; refreshMarketLib(); }
  catch (err) { b.textContent = 'Failed'; b.title = err.message; alert(err.message); }
  b.disabled = false;
});

export async function refreshMarketLib() {
  if (!mkt.loaded) return;
  const lib = await api('/api/market/library');
  $('#mLib').innerHTML = lib.map(m => `<div class="litem"><img src="${esc(m.thumb || '')}" alt="" loading="lazy">
    <div><div class="n" title="${esc(m.name)}">${esc(m.name)}</div>
    <div class="s">${esc(mkt.sources[m.source]?.label || m.source)} · ${esc(m.kind)} · ${esc(m.licence)}${m.author ? ' · ' + esc(m.author) : ''}</div>
    <code title="Folder on this machine">${esc(m.path)}</code>
    <div class="row">${m.kind === 'model' ? `<button class="primary sm" data-fin="${esc(m.source)}|${esc(m.id)}">Finish in studio</button><a class="ghost sm" href="/library/${esc(m.source)}/${esc(m.id)}/${esc(m.main)}" download>File</a>` : `<a class="ghost sm" href="/library/${esc(m.source)}/${esc(m.id)}/${esc(m.main)}" target="_blank">Preview</a>`}
    <a class="ghost sm" href="${esc(m.page)}" target="_blank" rel="noopener">Source</a><button class="ghost sm" data-rm="${esc(m.source)}|${esc(m.id)}">Remove</button></div></div></div>`).join('') || '<p class="hint">Grabbed assets show up here, with their licence and folder.</p>';
}
$('#mLib').addEventListener('click', async e => {
  const fin = e.target.closest('[data-fin]'), rm = e.target.closest('[data-rm]');
  if (fin) {
    const [source, id] = fin.dataset.fin.split('|'), fd = new FormData();
    fd.append('source', source); fd.append('id', id); fd.append('kind', 'prop'); fd.append('height', '0');
    fin.disabled = true; fin.textContent = 'Queued';
    try { const job = await api('/api/market/import', { method: 'POST', body: fd }); showTab('studio'); await refreshLibrary(); openJob(job.id); }
    catch (err) { fin.disabled = false; fin.textContent = 'Finish in studio'; alert(err.message); }
  }
  if (rm && confirm('Delete this download from your library?')) {
    const [source, id] = rm.dataset.rm.split('|'); await api(`/api/market/library/${source}/${id}`, { method: 'DELETE' }); refreshMarketLib(); if (mkt.items.length) marketSearch();
  }
});
