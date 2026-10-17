import * as THREE from 'three';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import { RoomEnvironment } from 'three/addons/environments/RoomEnvironment.js';

const $ = s => document.querySelector(s);
const api = (p, o) => fetch(p, o).then(async r => { if (!r.ok) throw new Error((await r.json().catch(() => ({}))).detail || r.statusText); return r.json(); });
let cfg, srcFile = null, crop = null, kind = 'character', current = null, variant = 1, pollTimer = null;
const views = {};

// ---------------- config / engines ----------------
async function loadConfig() {
  cfg = await api('/api/config');
  $('#engineChips').innerHTML = Object.entries(cfg.engines).map(([k, e]) => `<span class="chip ${e.ready ? 'ready' : ''}" title="${e.label}: ${e.ready ? 'ready' : 'needs ' + e.key}">${k}</span>`).join('');
  const sel = $('#engine'), was = sel.value;
  sel.innerHTML = Object.entries(cfg.engines).map(([k, e]) => `<option value="${k}" ${e.ready ? '' : 'disabled'}>${e.label}${e.ready ? '' : ' (no key)'}</option>`).join('');
  const firstReady = Object.keys(cfg.engines).find(k => cfg.engines[k].ready && k !== 'triposr') || 'triposr';
  sel.value = was && cfg.engines[was]?.ready ? was : firstReady;
  $('#kinds').innerHTML = Object.entries(cfg.presets).map(([k, p]) => `<button data-kind="${k}" class="${k === kind ? 'on' : ''}" title="${p.label}">${k}</button>`).join('');
  applyPreset(); engineChanged();
}
function applyPreset() {
  const p = cfg.presets[kind];
  $('#height').value = p.height; $('#polys').value = p.polys; $('#texture').value = p.texture; $('#origin').value = p.origin;
}
function engineChanged() {
  const e = cfg.engines[$('#engine').value];
  $('#engineNote').textContent = `${e.cost}${e.multiview ? ' · accepts back/left/right views' : ''}${$('#engine').value === 'triposr' ? ' · offline preview quality, not final' : ''}`;
  $('#views').hidden = !e.multiview;
  // Local engines take minutes per variant; start at one.
  if (!e.key) $('#variants').value = '1';
}
$('#kinds').addEventListener('click', e => { const b = e.target.closest('button'); if (!b) return; kind = b.dataset.kind; [...$('#kinds').children].forEach(x => x.classList.toggle('on', x === b)); applyPreset(); });
$('#engine').addEventListener('change', engineChanged);

// ---------------- keys ----------------
$('#keysBtn').onclick = () => $('#keysDlg').showModal();
$('#keysForm').addEventListener('submit', async e => {
  if (e.submitter?.value !== 'save') return;
  const fd = new FormData($('#keysForm'));
  await api('/api/keys', { method: 'POST', body: fd });
  $('#keysForm').reset(); loadConfig();
});

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
  $('.dropHint').hidden = !isGlb ? true : false;
  $('#stage').hidden = isGlb; $('#srcTools').hidden = isGlb;
  if (isGlb) $('.dropHint').innerHTML = `<b>${f.name}</b><br><small>will be finished, not regenerated</small>`;
  else $('#srcImg').src = URL.createObjectURL(f);
  if (!$('#name').value) $('#name').placeholder = f.name.replace(/\.[^.]+$/, '');
  $('#go').disabled = false;
  $('#go').textContent = isGlb ? 'Finish model' : 'Generate';
}
let dragStart = null;
const wrap = $('.imgwrap');
function imgRect() { return $('#srcImg').getBoundingClientRect(); }
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
  if (!isGlb && cfg.engines[$('#engine').value].multiview) for (const [k, f] of Object.entries(views)) fd.append(k, f);
  fd.append('options', JSON.stringify({
    name: $('#name').value || undefined, kind, engine: $('#engine').value, quality: $('#quality').value, variants: +$('#variants').value,
    height: +$('#height').value, polys: +$('#polys').value, texture: +$('#texture').value, origin: $('#origin').value,
    pose: $('#pose').value || undefined, pbr: $('#pbr').checked, delight: $('#delight').checked, keep_background: $('#keepBg').checked, crop,
  }));
  $('#go').disabled = true;
  try { const job = await api('/api/jobs', { method: 'POST', body: fd }); await refreshLibrary(); openJob(job.id); }
  catch (e) { alert(e.message); }
  $('#go').disabled = false;
};

// ---------------- library ----------------
async function refreshLibrary() {
  const list = await api('/api/jobs');
  $('#jobs').innerHTML = list.map(j => {
    const r = j.results?.[0], thumb = r ? `/files/${j.id}/${r.renders}/three_quarter.png` : '';
    return `<button class="job ${current?.id === j.id ? 'sel' : ''}" data-id="${j.id}">
      <div class="thumb" style="${thumb ? `background-image:url('${thumb}?t=${j.created}')` : ''}">${thumb ? '' : j.status}</div>
      <div class="meta"><span>${esc(j.name)}</span><span class="st ${j.status}">${j.engine}</span></div></button>`;
  }).join('') || '<p class="hint">Nothing yet.</p>';
}
const esc = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
$('#jobs').addEventListener('click', e => { const b = e.target.closest('.job'); if (b) openJob(b.dataset.id); });

async function openJob(id) {
  clearTimeout(pollTimer);
  const job = await api('/api/jobs/' + id);
  const fresh = current?.id !== id || current.status !== job.status || current.results.length !== job.results.length;
  current = job;
  document.querySelectorAll('.job').forEach(b => b.classList.toggle('sel', b.dataset.id === id));
  $('#detail').hidden = false; $('#empty').hidden = true;
  $('#log').textContent = job.log.join('\n'); $('#log').scrollTop = 1e9;
  const busy = job.status === 'running' || job.status === 'queued';
  $('#progress').hidden = !busy;
  const mins = Math.floor((Date.now() / 1000 - job.created) / 60);
  $('#progressText').textContent = job.status === 'queued' ? 'queued' : `${job.stage} · ${mins} min elapsed`;
  $('#variantTabs').innerHTML = job.results.length > 1 ? job.results.map(r => `<button data-v="${r.variant}" class="${r.variant === variant ? 'on' : ''}">v${r.variant}</button>`).join('') : '';
  if (!job.results.find(r => r.variant === variant)) variant = job.results[0]?.variant || 1;
  if (fresh) showVariant();
  $('#rfHeight').value = job.height; $('#rfPolys').value = job.polys; $('#rfRotate').value = 0; $('#rfOrigin').value = job.origin;
  if (busy) pollTimer = setTimeout(() => openJob(id), 2000); else refreshLibrary();
}
$('#variantTabs').addEventListener('click', e => { const b = e.target.closest('button'); if (!b) return; variant = +b.dataset.v; [...$('#variantTabs').children].forEach(x => x.classList.toggle('on', x === b)); showVariant(); });

function showVariant() {
  const r = current.results.find(x => x.variant === variant);
  $('.actions').hidden = $('#refinishBox').hidden = !r;
  if (!r) { $('#stats').innerHTML = current.error ? `<span class="warn">${esc(current.error)}</span>` : ''; clearModel(); return; }
  const s = r.stats, base = `/files/${current.id}/`;
  $('#stats').innerHTML = [
    ['tris', s.tris.toLocaleString()], ['size', s.dimensions_m.map(v => v.toFixed(2)).join(' × ') + ' m'],
    ['texture', s.textures?.map(t => t[1][0] + 'px').join(', ') || (s.vertex_colors ? 'vertex colour' : 'none')],
    ['engine', r.meta.engine], ['floaters removed', s.floaters_removed], ['open edges', s.non_manifold_edges],
  ].map(([k, v]) => `<span>${k} <b>${esc(v)}</b></span>`).join('') + (s.warnings || []).map(w => `<span class="warn">${esc(w)}</span>`).join('') +
    (current.prep?.notes || []).map(w => `<span class="warn">${esc(w)}</span>`).join('');
  $('#dlGlb').href = base + r.glb + '?t=' + Date.now(); $('#dlGlb').download = r.glb.split('/').pop();
  $('#dlRaw').href = base + r.raw; $('#dlRaw').download = current.name + '-raw.glb';
  $('#dlSheet').href = base + r.renders + '/sheet.jpg';
  $('#dlRig').hidden = !r.rigged; $('#rigBtn').textContent = r.rigged ? 'Re-rig' : 'Rig + animate';
  if (r.rigged) { $('#dlRig').href = base + r.rigged + '?t=' + Date.now(); $('#dlRig').download = r.rigged.split('/').pop(); }
  loadModel(base + (r.rigged || r.glb) + '?t=' + Date.now());
}

$('#refinish').onclick = async () => {
  const fd = new FormData();
  fd.append('options', JSON.stringify({ height: +$('#rfHeight').value, polys: +$('#rfPolys').value, rotate: (+$('#rfRotate').value || 0) + (current.rotate || 0), origin: $('#rfOrigin').value }));
  await api(`/api/jobs/${current.id}/refinish`, { method: 'POST', body: fd });
  current.status = 'queued'; setTimeout(() => openJob(current.id), 500);
};
$('#rigBtn').onclick = async () => { await api(`/api/jobs/${current.id}/rig`, { method: 'POST' }); current.status = 'queued'; setTimeout(() => openJob(current.id), 500); };
$('#rerun').onclick = async () => { await api(`/api/jobs/${current.id}/rerun`, { method: 'POST' }); current.status = 'queued'; setTimeout(() => openJob(current.id), 500); };

// ---------------- viewer ----------------
const canvas = $('#view');
const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true });
renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
renderer.toneMapping = THREE.ACESFilmicToneMapping; renderer.toneMappingExposure = 1.0;
renderer.shadowMap.enabled = true; renderer.shadowMap.type = THREE.PCFSoftShadowMap;
const scene = new THREE.Scene();
const pmrem = new THREE.PMREMGenerator(renderer);
const envTex = pmrem.fromScene(new RoomEnvironment(), 0.04).texture;
scene.environment = envTex;
const camera = new THREE.PerspectiveCamera(32, 1, 0.001, 100);
const controls = new OrbitControls(camera, canvas);
controls.enableDamping = true; controls.autoRotate = true; controls.autoRotateSpeed = 1.2;
const key = new THREE.DirectionalLight(0xfff3e6, 2.2); key.castShadow = true; key.shadow.mapSize.set(2048, 2048); key.shadow.bias = -0.0004;
const rimA = new THREE.DirectionalLight(0x38f5c6, 0), rimB = new THREE.DirectionalLight(0xff4fb6, 0);
scene.add(key, key.target, rimA, rimB);
const ground = new THREE.Mesh(new THREE.CircleGeometry(1, 64), new THREE.ShadowMaterial({ opacity: .45 }));
ground.rotation.x = -Math.PI / 2; ground.receiveShadow = true; scene.add(ground);
// dot-matrix floor
const dotGeo = new THREE.BufferGeometry(), pts = [];
for (let x = -30; x <= 30; x++) for (let z = -30; z <= 30; z++) if (x * x + z * z <= 900) pts.push(x / 30, 0, z / 30);
dotGeo.setAttribute('position', new THREE.Float32BufferAttribute(pts, 3));
const dots = new THREE.Points(dotGeo, new THREE.PointsMaterial({ color: 0x38f5c6, size: 1.6, sizeAttenuation: false, transparent: true, opacity: .22 }));
scene.add(dots);
let model = null, mode = 'texture', lightMode = 'studio', origMats = new Map();
const clay = new THREE.MeshStandardMaterial({ color: 0xb9b4ad, roughness: .62 });
const wireMat = new THREE.MeshBasicMaterial({ color: 0x38f5c6, wireframe: true, transparent: true, opacity: .55 });

let mixer = null, clips = [];
const clock = new THREE.Clock();
function playClip(name) {
  if (!mixer) return;
  mixer.stopAllAction();
  const c = clips.find(c => c.name === name);
  if (c) mixer.clipAction(c).play();
}
$('#clip').onchange = e => playClip(e.target.value);
function clearModel() { mixer = null; clips = []; $('#clip').hidden = true; if (model) { scene.remove(model); model.traverse(o => { o.geometry?.dispose(); }); model = null; origMats.clear(); } }
function loadModel(url) {
  new GLTFLoader().load(url, g => {
    clearModel(); model = g.scene;
    model.traverse(o => { if (o.isMesh) { o.castShadow = o.receiveShadow = true; origMats.set(o, o.material); } });
    scene.add(model);
    // Measure the meshes' own geometry: a skinned mesh's object bounds follow the skeleton, not the body.
    const box = new THREE.Box3(); model.updateMatrixWorld(true);
    model.traverse(o => { if (o.isMesh) { o.geometry.computeBoundingBox(); box.union(o.geometry.boundingBox.clone().applyMatrix4(o.isSkinnedMesh ? o.bindMatrix.clone().premultiply(o.matrixWorld) : o.matrixWorld)); if (o.isSkinnedMesh) o.frustumCulled = false; } });
    const size = box.getSize(new THREE.Vector3()), c = box.getCenter(new THREE.Vector3());
    const r = Math.max(size.x, size.y, size.z);
    ground.position.y = dots.position.y = box.min.y + r * 0.001;
    ground.scale.setScalar(r * 1.2); dots.scale.setScalar(r * 1.4);
    controls.target.copy(c); camera.near = r / 100; camera.far = r * 50; camera.updateProjectionMatrix();
    camera.position.copy(c).add(new THREE.Vector3(r * .9, r * .35, r * 2.2));
    const sc = key.shadow.camera; sc.left = sc.bottom = -r; sc.right = sc.top = r; sc.near = r * .1; sc.far = r * 8; sc.updateProjectionMatrix();
    key.position.copy(c).add(new THREE.Vector3(-r * 1.5, r * 2.5, r * 2)); key.target.position.copy(c);
    rimA.position.copy(c).add(new THREE.Vector3(r * 2, r, -r * 2)); rimB.position.copy(c).add(new THREE.Vector3(-r * 2, r * .6, -r * 2));
    applyMode(); applyLight();
    clips = g.animations || [];
    if (clips.length) {
      mixer = new THREE.AnimationMixer(model);
      const names = clips.map(c => c.name).sort();
      const pick = names.includes('Idle_Loop') ? 'Idle_Loop' : names[0];
      $('#clip').innerHTML = names.map(n => `<option ${n === pick ? 'selected' : ''}>${esc(n)}</option>`).join('');
      $('#clip').hidden = false; playClip(pick);
    }
  });
}
function applyMode() {
  if (!model) return;
  model.traverse(o => { if (o.isMesh) o.material = mode === 'clay' ? clay : mode === 'wire' ? wireMat : origMats.get(o); });
}
function applyLight() {
  const neon = lightMode === 'neon';
  scene.environment = envTex; renderer.toneMappingExposure = neon ? .75 : 1;
  key.intensity = neon ? 1.1 : 2.2; rimA.intensity = neon ? 3.5 : .5; rimB.intensity = neon ? 2.5 : .3;
  scene.environmentIntensity = neon ? .35 : 1;
}
$('#modes').addEventListener('click', e => { const b = e.target.closest('button'); if (!b) return; mode = b.dataset.mode; [...$('#modes').children].forEach(x => x.classList.toggle('on', x === b)); applyMode(); });
$('#lights').addEventListener('click', e => { const b = e.target.closest('button'); if (!b) return; lightMode = b.dataset.light; [...$('#lights').children].forEach(x => x.classList.toggle('on', x === b)); applyLight(); });
$('#spin').onclick = () => { controls.autoRotate = !controls.autoRotate; $('#spin').classList.toggle('on', controls.autoRotate); };

function resize() {
  const w = canvas.parentElement.clientWidth, h = canvas.parentElement.clientHeight;
  if (canvas.width !== Math.floor(w * renderer.getPixelRatio()) || canvas.height !== Math.floor(h * renderer.getPixelRatio())) {
    renderer.setSize(w, h, false); camera.aspect = w / h; camera.updateProjectionMatrix();
  }
}
renderer.setAnimationLoop(() => { const dt = clock.getDelta(); mixer?.update(dt); resize(); controls.update(); renderer.render(scene, camera); });

await loadConfig();
await refreshLibrary();
const first = (await api('/api/jobs')).find(j => j.status === 'done');
if (first) openJob(first.id);
