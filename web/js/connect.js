// "Connect AI": copy-paste MCP setup for Claude, Codex, Cursor, VS Code, Gemini CLI and the rest.
import { $, api, esc } from './api.js';

$('#connectBtn').onclick = async () => {
  const info = await api('/api/connect');
  $('#connectUrl').textContent = info.url;
  $('#connectList').innerHTML = info.clients.map((c, i) => `<details ${i === 0 ? 'open' : ''}><summary>${esc(c.name)} <small>${esc(c.where)}</small></summary>
    <pre class="snip">${esc(c.text)}</pre><button class="ghost sm" data-copy="${i}">Copy</button></details>`).join('');
  $('#connectList').onclick = e => {
    const b = e.target.closest('[data-copy]'); if (!b) return;
    navigator.clipboard.writeText(info.clients[+b.dataset.copy].text).then(() => { b.textContent = 'Copied'; setTimeout(() => b.textContent = 'Copy', 1500); });
  };
  $('#connectDlg').showModal();
};
