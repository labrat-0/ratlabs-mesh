// The library strip and the open job: stats, downloads, variants, refinish / rig / rerun.
// Updates arrive live over /api/events; a slow poll covers jobs run from another process (the CLI).
import { $, api, esc, pick } from './api.js';
import { state } from './state.js';
import { clearModel, loadModel } from './viewer.js';

let pollTimer = null, libTimer = null;

export async function refreshLibrary() {
  const list = await api('/api/jobs');
  $('#jobs').innerHTML = list.map(j => {
    const r = j.results?.[0], thumb = r ? `/files/${j.id}/${r.renders}/three_quarter.png` : '';
    const ai = j.by && j.by !== 'studio' && j.by !== 'cli';
    return `<button class="job ${state.current?.id === j.id ? 'sel' : ''}" data-id="${j.id}">
      <div class="thumb" style="${thumb ? `background-image:url('${thumb}?t=${j.created}')` : ''}">${thumb ? '' : j.status}${ai ? `<span class="by" title="Made by ${esc(j.by)}">AI</span>` : ''}</div>
      <div class="meta"><span>${esc(j.name)}</span><span class="st ${j.status}">${j.engine}</span></div></button>`;
  }).join('') || starterOffer();
  return list;
}

// A new library offers the starter pack instead of sitting empty.
function starterOffer() {
  const about = state.cfg?.starter?.about || '30 free CC0 props';
  return `<div class="starter"><p class="hint">Nothing here yet. Drop some art above, or start with the starter pack: ${esc(about)}.</p>
    <button id="starterBtn" class="primary sm">Load starter pack</button></div>`;
}
$('#jobs').addEventListener('click', async e => {
  const b = e.target.closest('#starterBtn'); if (!b) return;
  b.disabled = true; b.textContent = 'Downloading starter pack…';
  try { await api('/api/starter', { method: 'POST' }); const list = await refreshLibrary(); if (list[0]) openJob(list[0].id); }
  catch (err) { b.disabled = false; b.textContent = 'Load starter pack'; alert(err.message); }
});

// Many job events arrive per second while an engine logs; refresh the strip at most every 1.5 s.
export function refreshLibrarySoon() {
  if (libTimer) return;
  libTimer = setTimeout(() => { libTimer = null; refreshLibrary(); }, 1500);
}

$('#jobs').addEventListener('click', e => { const b = e.target.closest('.job'); if (b) openJob(b.dataset.id); });

export async function openJob(id, opts = {}) {
  clearTimeout(pollTimer);
  const job = await api('/api/jobs/' + id);
  const prev = state.current;
  const fresh = opts.reload || prev?.id !== id || prev.status !== job.status || prev.results.length !== job.results.length;
  state.current = job;
  if (opts.variant) state.variant = opts.variant;
  if (location.hash !== '#job=' + id) history.replaceState(null, '', '#job=' + id);
  document.querySelectorAll('.job').forEach(b => b.classList.toggle('sel', b.dataset.id === id));
  $('#detail').hidden = false; $('#empty').hidden = true;
  $('#log').textContent = job.log.join('\n'); $('#log').scrollTop = 1e9;
  const busy = job.status === 'running' || job.status === 'queued';
  $('#progress').hidden = !busy;
  const mins = Math.floor((Date.now() / 1000 - job.created) / 60);
  const who = job.by && job.by !== 'studio' && job.by !== 'cli' ? ` · ${job.by}` : '';
  $('#progressText').textContent = job.status === 'queued' ? 'queued' + who : `${job.stage} · ${mins} min elapsed${who}`;
  $('#variantTabs').innerHTML = job.results.length > 1 ? job.results.map(r => `<button data-v="${r.variant}" class="${r.variant === state.variant ? 'on' : ''}">v${r.variant}</button>`).join('') : '';
  if (!job.results.find(r => r.variant === state.variant)) state.variant = job.results[0]?.variant || 1;
  if (fresh || opts.variant) showVariant();
  $('#rfHeight').value = job.height; $('#rfPolys').value = job.polys; $('#rfRotate').value = 0; $('#rfOrigin').value = job.origin;
  if (busy) pollTimer = setTimeout(() => openJob(id), 5000);
  else if (fresh) refreshLibrary();
}

$('#variantTabs').addEventListener('click', e => {
  const b = e.target.closest('button'); if (!b) return;
  state.variant = +b.dataset.v; pick($('#variantTabs'), b); showVariant();
});

function showVariant() {
  const cur = state.current, r = cur.results.find(x => x.variant === state.variant);
  $('.actions').hidden = $('#refinishBox').hidden = !r;
  if (!r) { $('#stats').innerHTML = cur.error ? `<span class="warn">${esc(cur.error)}</span>` : ''; clearModel(); return; }
  const s = r.stats, base = `/files/${cur.id}/`;
  $('#stats').innerHTML = [
    ['tris', (s.tris ?? 0).toLocaleString()], ['size', (s.dimensions_m || []).map(v => v.toFixed(2)).join(' × ') + ' m'],
    ['texture', s.textures?.map(t => t[1][0] + 'px').join(', ') || (s.vertex_colors ? 'vertex colour' : 'none')],
    ['engine', r.meta?.engine], ['floaters removed', s.floaters_removed], ['open edges', s.non_manifold_edges],
  ].map(([k, v]) => `<span>${k} <b>${esc(v)}</b></span>`).join('') + (s.warnings || []).map(w => `<span class="warn">${esc(w)}</span>`).join('') +
    (cur.prep?.notes || []).map(w => `<span class="warn">${esc(w)}</span>`).join('') +
    (cur.licence ? `<span>${esc(cur.licence)} · ${esc(cur.author || '')} · <a href="${esc(cur.source)}" target="_blank" rel="noopener">source</a></span>` : '');
  $('#rerun').hidden = !!cur.pack;
  $('#dlGlb').href = base + r.glb + '?t=' + Date.now(); $('#dlGlb').download = r.glb.split('/').pop();
  $('#dlRaw').href = base + r.raw; $('#dlRaw').download = cur.name + '-raw.glb';
  $('#dlSheet').href = base + r.renders + '/sheet.jpg';
  $('#dlRig').hidden = !r.rigged; $('#rigBtn').textContent = r.rigged ? 'Re-rig' : 'Rig + animate';
  if (r.rigged) { $('#dlRig').href = base + r.rigged + '?t=' + Date.now(); $('#dlRig').download = r.rigged.split('/').pop(); }
  loadModel(base + (r.rigged || r.glb) + '?t=' + Date.now());
}

/** A job changed somewhere (studio, AI client, worker): update what's on screen. */
export function onJobEvent(job) {
  refreshLibrarySoon();
  if (state.current?.id === job.id) openJob(job.id);
}

async function queue(path, body) {
  try { await api(`/api/jobs/${state.current.id}/${path}`, { method: 'POST', body }); }
  catch (e) { alert(e.message); return; }
  state.current.status = 'queued'; setTimeout(() => openJob(state.current.id), 300);
}
$('#refinish').onclick = () => {
  const fd = new FormData();
  fd.append('options', JSON.stringify({ height: +$('#rfHeight').value, polys: +$('#rfPolys').value, rotate: (+$('#rfRotate').value || 0) + (state.current.rotate || 0), origin: $('#rfOrigin').value }));
  queue('refinish', fd);
};
$('#rigBtn').onclick = () => queue('rig');
$('#rerun').onclick = () => queue('rerun');
