// The "New asset" panel: engines and presets, source art with a crop box, cut-out preview, submit.
import { $, api, pick } from './api.js';
import { state } from './state.js';
import { openJob, refreshLibrary } from './job.js';

let srcFile = null, crop = null;
const views = {};

export async function loadConfig() {
  const cfg = state.cfg = await api('/api/config');
  $('#engineChips').innerHTML = Object.entries(cfg.engines).map(([k, e]) => `<span class="chip ${e.ready ? 'ready' : ''}" title="${e.label}: ${e.ready ? 'ready' : (e.setup || 'needs ' + e.key)}">${k}</span>`).join('');
  const sel = $('#engine'), was = sel.value;
  sel.innerHTML = Object.entries(cfg.engines).map(([k, e]) => `<option value="${k}" ${e.ready ? '' : 'disabled'}>${e.label}${e.ready ? '' : (e.key ? ' (no key)' : ' (not installed)')}</option>`).join('');
  const firstReady = Object.keys(cfg.engines).find(k => cfg.engines[k].ready && k !== 'triposr') || 'triposr';
  sel.value = was && cfg.engines[was]?.ready ? was : firstReady;
  $('#kinds').innerHTML = Object.entries(cfg.presets).map(([k, p]) => `<button data-kind="${k}" class="${k === state.kind ? 'on' : ''}" title="${p.label}">${k}</button>`).join('');
  applyPreset(); engineChanged();
}

function applyPreset() {
  const p = state.cfg.presets[state.kind];
  $('#height').value = p.height; $('#polys').value = p.polys; $('#texture').value = p.texture; $('#origin').value = p.origin;
}

function engineChanged() {
  const e = state.cfg.engines[$('#engine').value];
  if (!e) return;
  $('#engineNote').textContent = e.ready
    ? `${e.cost}${e.multiview ? ' · accepts back/left/right views' : ''}${$('#engine').value === 'triposr' ? ' · offline preview quality, not final' : ''}`
    : `Not set up yet: ${e.setup || 'add its key under Keys'}`;
  $('#views').hidden = !e.multiview;
  // Local engines take minutes per variant; start at one.
  if (e.local) $('#variants').value = '1';
  goLabel();
}

// The button says what pressing it will do: generate N variants, finish a GLB, or set up an engine first.
export function goLabel() {
  const e = state.cfg?.engines[$('#engine').value], go = $('#go');
  if (srcFile && /\.glb$/i.test(srcFile.name)) { go.textContent = 'Finish model'; go.disabled = false; return; }
  const n = +$('#variants').value;
  go.textContent = !e?.ready ? 'Add a key under Keys first' : n > 1 ? `Generate ${n} variants` : 'Generate';
  go.disabled = !srcFile || !e?.ready;
}
$('#variants').addEventListener('change', goLabel);
$('#kinds').addEventListener('click', e => { const b = e.target.closest('button'); if (!b) return; state.kind = b.dataset.kind; pick($('#kinds'), b); applyPreset(); });
$('#engine').addEventListener('change', engineChanged);

// ---------------- source art + crop box ----------------
const drop = $('#drop'), fileInput = $('#file');
drop.onclick = e => { if (!srcFile || e.target === drop) fileInput.click(); };
drop.onkeydown = e => { if (e.key === 'Enter' || e.key === ' ') fileInput.click(); };
drop.ondragover = e => { e.preventDefault(); drop.classList.add('over'); };
drop.ondragleave = () => drop.classList.remove('over');
drop.ondrop = e => { e.preventDefault(); drop.classList.remove('over'); if (e.dataTransfer.files[0]) setSource(e.dataTransfer.files[0]); };
fileInput.onchange = () => fileInput.files[0] && setSource(fileInput.files[0]);

function setSource(f) {
  srcFile = f; crop = null; $('#cropBox').hidden = true; $('#cutImg').hidden = true; $('#stage').classList.remove('withCut'); $('#prepNote').hidden = true;
  const isGlb = /\.glb$/i.test(f.name);
  $('.dropHint').hidden = !isGlb;
  $('#stage').hidden = isGlb; $('#srcTools').hidden = isGlb;
  if (isGlb) $('.dropHint').innerHTML = `<b>${f.name}</b><br><small>will be finished, not regenerated</small>`;
  else $('#srcImg').src = URL.createObjectURL(f);
  if (!$('#name').value) $('#name').placeholder = f.name.replace(/\.[^.]+$/, '');
  goLabel();
}

let dragStart = null;
const wrap = $('.imgwrap');
const imgRect = () => $('#srcImg').getBoundingClientRect();
const frac = (e, R) => [Math.min(1, Math.max(0, (e.clientX - R.x) / R.width)), Math.min(1, Math.max(0, (e.clientY - R.y) / R.height))];
wrap.addEventListener('pointerdown', e => { e.preventDefault(); e.stopPropagation(); wrap.setPointerCapture(e.pointerId); dragStart = frac(e, imgRect()); });
wrap.addEventListener('pointermove', e => {
  if (!dragStart) return; const p = frac(e, imgRect());
  crop = [Math.min(dragStart[0], p[0]), Math.min(dragStart[1], p[1]), Math.max(dragStart[0], p[0]), Math.max(dragStart[1], p[1])];
  drawCrop();
});
wrap.addEventListener('pointerup', () => { dragStart = null; if (crop && (crop[2] - crop[0] < .03 || crop[3] - crop[1] < .03)) { crop = null; $('#cropBox').hidden = true; } });
wrap.addEventListener('click', e => e.stopPropagation());
function drawCrop() { // percentages of the image, so the box survives layout changes
  const b = $('#cropBox'); b.hidden = false;
  Object.assign(b.style, { left: crop[0] * 100 + '%', top: crop[1] * 100 + '%', width: (crop[2] - crop[0]) * 100 + '%', height: (crop[3] - crop[1]) * 100 + '%' });
}
$('#clearCrop').onclick = () => { crop = null; $('#cropBox').hidden = true; };
$('#previewCut').onclick = async () => {
  const b = $('#previewCut'); b.disabled = true; b.textContent = 'Cutting…';
  try {
    const fd = new FormData(); fd.append('image', srcFile); if (crop) fd.append('crop', JSON.stringify(crop));
    const r = await fetch('/api/cutout', { method: 'POST', body: fd });
    if (!r.ok) throw new Error((await r.json()).detail);
    const info = JSON.parse(r.headers.get('X-Prep'));
    $('#cutImg').src = URL.createObjectURL(await r.blob()); $('#cutImg').hidden = false; $('#stage').classList.add('withCut');
    $('#prepNote').hidden = !info.notes.length; $('#prepNote').textContent = info.notes.join(' ');
  } catch (err) { $('#prepNote').hidden = false; $('#prepNote').textContent = err.message; }
  b.disabled = false; b.textContent = 'Preview cut-out';
};
document.querySelectorAll('.vdrop input').forEach(inp => inp.onchange = () => {
  const f = inp.files[0], l = inp.parentElement; if (!f) return;
  views[inp.dataset.side] = f; l.classList.add('has'); l.style.backgroundImage = `url(${URL.createObjectURL(f)})`;
});

// ---------------- submit ----------------
$('#go').onclick = async () => {
  const fd = new FormData(), isGlb = /\.glb$/i.test(srcFile.name);
  fd.append(isGlb ? 'glb' : 'image', srcFile);
  if (!isGlb && state.cfg.engines[$('#engine').value].multiview) for (const [k, f] of Object.entries(views)) fd.append(k, f);
  fd.append('options', JSON.stringify({
    name: $('#name').value || undefined, kind: state.kind, engine: $('#engine').value, quality: $('#quality').value, variants: +$('#variants').value,
    height: +$('#height').value, polys: +$('#polys').value, texture: +$('#texture').value, origin: $('#origin').value,
    pose: $('#pose').value || undefined, pbr: $('#pbr').checked, delight: $('#delight').checked, keep_background: $('#keepBg').checked, crop,
  }));
  $('#go').disabled = true;
  try { const job = await api('/api/jobs', { method: 'POST', body: fd }); await refreshLibrary(); openJob(job.id); }
  catch (e) { alert(e.message); }
  goLabel();
};
