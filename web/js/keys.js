// The Keys dialog. Keys go to keys.env on this machine (mode 600) and never come back to the page.
import { $, api } from './api.js';

export function initKeys(onSaved) {
  $('#keysBtn').onclick = () => $('#keysDlg').showModal();
  $('#keysForm').addEventListener('submit', async e => {
    if (e.submitter?.value !== 'save') return;
    await api('/api/keys', { method: 'POST', body: new FormData($('#keysForm')) });
    $('#keysForm').reset(); onSaved();
  });
}
