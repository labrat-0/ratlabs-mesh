// Live updates from /api/events: job progress, AI activity, and "look at this" requests from AI clients.
import { $, esc } from './api.js';

const TOOL_LABEL = {
  connected: 'connected', mesh_status: 'checking engines', preview_cutout: 'previewing a cut-out', generate_model: 'generating a model',
  import_model: 'finishing a model', get_job: 'following a job', list_jobs: 'browsing the library', view_model: 'looking at a model',
  show_in_studio: 'showing you a model', refinish_model: 'refinishing', rig_model: 'rigging', rerun_job: 'running a job again',
  concept_art: 'drawing concept art', market_search: 'searching the Market', market_grab: 'downloading an asset',
  market_library: 'checking downloads', market_finish: 'finishing a Market model',
};
let agentTimer = null;

function agent(client, tool) {
  const chip = $('#agent');
  chip.hidden = false;
  chip.innerHTML = `<span class="pulse"></span><b>${esc(client)}</b> ${esc(TOOL_LABEL[tool] || tool)}`;
  clearTimeout(agentTimer);
  agentTimer = setTimeout(() => { chip.hidden = true; }, 20000);
}

/** handlers: {job(job), show(ev), config(), library()} */
export function connectLive(handlers) {
  const es = new EventSource('/api/events');
  es.onmessage = m => {
    const ev = JSON.parse(m.data);
    if (ev.type === 'job') handlers.job(ev.job);
    else if (ev.type === 'show') handlers.show(ev);
    else if (ev.type === 'agent') agent(ev.client, ev.tool);
    else if (ev.type === 'config') handlers.config();
    else if (ev.type === 'library') handlers.library();
  };
  es.onerror = () => { $('#live').classList.add('off'); $('#live').title = 'Live updates reconnecting…'; };
  es.onopen = () => { $('#live').classList.remove('off'); $('#live').title = 'Live: AI clients and other windows update this page'; };
}
