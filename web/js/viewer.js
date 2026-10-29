// The 3D viewer: texture / clay / wire modes, studio / neon lighting, turntable, animation clips.
import * as THREE from 'three';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import { RoomEnvironment } from 'three/addons/environments/RoomEnvironment.js';
import { $, esc, pick } from './api.js';

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
const rimA = new THREE.DirectionalLight(0x39ff88, 0), rimB = new THREE.DirectionalLight(0xf2f2f2, 0);
scene.add(key, key.target, rimA, rimB);
const ground = new THREE.Mesh(new THREE.CircleGeometry(1, 64), new THREE.ShadowMaterial({ opacity: .45 }));
ground.rotation.x = -Math.PI / 2; ground.receiveShadow = true; scene.add(ground);
// dot-matrix floor
const dotGeo = new THREE.BufferGeometry(), pts = [];
for (let x = -30; x <= 30; x++) for (let z = -30; z <= 30; z++) if (x * x + z * z <= 900) pts.push(x / 30, 0, z / 30);
dotGeo.setAttribute('position', new THREE.Float32BufferAttribute(pts, 3));
const dots = new THREE.Points(dotGeo, new THREE.PointsMaterial({ color: 0x39ff88, size: 1.6, sizeAttenuation: false, transparent: true, opacity: .22 }));
scene.add(dots);
let model = null, mode = 'texture', lightMode = 'studio';
const origMats = new Map();
const clay = new THREE.MeshStandardMaterial({ color: 0xb6b6b6, roughness: .62 });
const wireMat = new THREE.MeshBasicMaterial({ color: 0x39ff88, wireframe: true, transparent: true, opacity: .55 });

let mixer = null, clips = [];
const clock = new THREE.Clock();
function playClip(name) {
  if (!mixer) return;
  mixer.stopAllAction();
  const c = clips.find(c => c.name === name);
  if (c) mixer.clipAction(c).play();
}
$('#clip').onchange = e => playClip(e.target.value);

export function clearModel() {
  mixer = null; clips = []; $('#clip').hidden = true;
  if (model) { scene.remove(model); model.traverse(o => { o.geometry?.dispose(); }); model = null; origMats.clear(); }
}

export function loadModel(url) {
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
      const first = names.includes('Idle_Loop') ? 'Idle_Loop' : names.find(n => /idle/i.test(n)) || names[0];
      $('#clip').innerHTML = names.map(n => `<option ${n === first ? 'selected' : ''}>${esc(n)}</option>`).join('');
      $('#clip').hidden = false; playClip(first);
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

// Also driven by AI clients through show_in_studio.
export function setMode(m) {
  const b = $(`#modes [data-mode="${m}"]`); if (!b) return;
  mode = m; pick($('#modes'), b); applyMode();
}
export function setLight(l) {
  const b = $(`#lights [data-light="${l}"]`); if (!b) return;
  lightMode = l; pick($('#lights'), b); applyLight();
}
$('#modes').addEventListener('click', e => { const b = e.target.closest('button'); if (b) setMode(b.dataset.mode); });
$('#lights').addEventListener('click', e => { const b = e.target.closest('button'); if (b) setLight(b.dataset.light); });
$('#spin').onclick = () => { controls.autoRotate = !controls.autoRotate; $('#spin').classList.toggle('on', controls.autoRotate); };

function resize() {
  const w = canvas.parentElement.clientWidth, h = canvas.parentElement.clientHeight;
  if (canvas.width !== Math.floor(w * renderer.getPixelRatio()) || canvas.height !== Math.floor(h * renderer.getPixelRatio())) {
    renderer.setSize(w, h, false); camera.aspect = w / h; camera.updateProjectionMatrix();
  }
}
renderer.setAnimationLoop(() => { const dt = clock.getDelta(); mixer?.update(dt); resize(); controls.update(); renderer.render(scene, camera); });
