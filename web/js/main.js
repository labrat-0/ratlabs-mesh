// RatLabs Mesh studio: wires the modules together.
import { $, pick } from './api.js';
import { state } from './state.js';
import { loadConfig } from './studio.js';
import { onJobEvent, openJob, refreshLibrary } from './job.js';
import { marketInit, refreshMarketLib, reloadSources } from './market.js';
import { initKeys } from './keys.js';
import { connectLive } from './live.js';
import { setLight, setMode } from './viewer.js';
import './connect.js';

function showTab(t) {
  pick($('#tabs'), $(`#tabs [data-tab="${t}"]`));
  document.querySelector('main').hidden = t !== 'studio';
  $('#market').hidden = t !== 'market';
  if (t === 'market') marketInit(showTab);
}
$('#tabs').addEventListener('click', e => { const b = e.target.closest('button'); if (b) showTab(b.dataset.tab); });

initKeys(() => { loadConfig(); reloadSources(); });

connectLive({
  job: onJobEvent,
  // An AI client asked the user to look: bring the job up, as it asked.
  show: async ev => {
    showTab('studio');
    if (state.current?.id !== ev.id) { await refreshLibrary(); }
    await openJob(ev.id, { variant: ev.variant, reload: state.current?.id !== ev.id });
    if (ev.mode) setMode(ev.mode);
    if (ev.light) setLight(ev.light);
    $('#viewerPanel').scrollIntoView({ behavior: 'smooth', block: 'start' });
  },
  config: () => { loadConfig(); reloadSources(); },
  library: refreshMarketLib,
});

await loadConfig();
const list = await refreshLibrary();
// CLI jobs run in their own process and send no events; pick them up now and then.
setInterval(refreshLibrary, 30000);
const hash = new URLSearchParams(location.hash.slice(1));
if (location.hash === '#market') showTab('market');
else if (hash.get('job')) openJob(hash.get('job')).catch(() => {});
else { const first = list.find(j => j.status === 'done'); if (first) openJob(first.id); }
window.addEventListener('hashchange', () => { const id = new URLSearchParams(location.hash.slice(1)).get('job'); if (id && id !== state.current?.id) { showTab('studio'); openJob(id); } });
