import { apiFetch } from '../services/api.js';
import { showToast } from '../services/toast.js';

const form = document.getElementById('user-edit-form');
if (form) {
  const status = document.getElementById('user-edit-status');
  form.addEventListener('submit', async (event) => {
    event.preventDefault();
    const userId = form.dataset.userId;
    if (!userId) return;
    const data = new FormData(form);
    const rolesRaw = (data.get('roles') || '').toString();
    const roles = rolesRaw.split(',').map((r) => r.trim()).filter(Boolean);
    const body = {
      display_name: (data.get('display_name') || '').toString(),
      email: (data.get('email') || '').toString(),
      roles,
      is_active: form.elements.namedItem('is_active').checked,
      department: (data.get('department') || '').toString(),
    };
    if (status) status.textContent = 'Saving…';
    try {
      await apiFetch('/users/' + encodeURIComponent(userId), {
        method: 'PUT',
        body: JSON.stringify(body),
      });
      if (status) status.textContent = 'Saved';
      showToast('User updated', 'success');
      setTimeout(() => window.location.reload(), 600);
    } catch (err) {
      const msg = err && err.message ? err.message : 'Failed to update user';
      if (status) status.textContent = '';
      showToast(msg, 'error');
    }
  });
}

const creditForm = document.getElementById('credit-grant-form');
if (creditForm) {
  const status = document.getElementById('credit-grant-status');
  creditForm.addEventListener('submit', async (event) => {
    event.preventDefault();
    const userId = creditForm.dataset.userId;
    if (!userId) return;
    const data = new FormData(creditForm);
    const usd = Number.parseFloat((data.get('usd') || '').toString());
    if (!Number.isFinite(usd) || usd <= 0) {
      showToast('Enter a positive dollar amount', 'error');
      return;
    }
    const reason = (data.get('reason') || '').toString().trim();
    const body = reason ? { usd, reason } : { usd };
    if (status) status.textContent = 'Granting…';
    try {
      const res = await apiFetch('/users/' + encodeURIComponent(userId) + '/credit', {
        method: 'POST',
        body: JSON.stringify(body),
      });
      // A repeated reason is a no-op, not a grant — say so rather than
      // reporting a success the ledger did not record.
      if (res && res.granted === false) {
        if (status) status.textContent = '';
        showToast('No-op: "' + res.reason + '" was already granted to this user', 'error');
        return;
      }
      if (status) status.textContent = 'Granted';
      showToast('Credit granted', 'success');
      setTimeout(() => window.location.reload(), 600);
    } catch (err) {
      const msg = err && err.message ? err.message : 'Failed to grant credit';
      if (status) status.textContent = '';
      showToast(msg, 'error');
    }
  });
}
