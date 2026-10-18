(() => {
  const invoke = window.__TAURI__?.core?.invoke;
  const $ = (selector) => document.querySelector(selector);
  const content = $('#content');
  const state = { serial: '', device: null, devices: [], tab: 'overview', telemetry: null, data: [], path: '/sdcard', screenTimer: null, toastTimer: null, lastTelemetry: 0 };
  const tabNames = { overview: 'Overview', screen: 'Screen share', messages: 'Messages', calls: 'Calls', photos: 'Photos', notifications: 'Notifications', storage: 'Storage', apps: 'Apps', clipboard: 'Clipboard', terminal: 'Terminal', wireless: 'Wireless debugging', firmware: 'Device controls' };
  const tabKinds = { messages: 'messages', calls: 'calls', photos: 'photos', notifications: 'notifications', apps: 'apps', storage: 'files', wireless: 'endpoints' };
  const escapeHtml = (v) => String(v ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  const val = (v, suffix = '') => v == null || v === '' ? 'Unavailable' : `${v}${suffix}`;
  const invokeOrFail = (...args) => { if (!invoke) throw new Error('Open this page inside the Server Launcher Tauri window.'); return invoke(...args); };

  function toast(message, error = false) {
    const el = $('#toast'); el.textContent = message; el.className = `toast show${error ? ' error' : ''}`;
    clearTimeout(state.toastTimer); state.toastTimer = setTimeout(() => { el.className = 'toast'; }, 3300);
  }
  function setConnection(ok, label) { const el = $('#connection-state'); el.className = `connection ${ok ? 'online' : 'offline'}`; el.lastElementChild.textContent = label; }
  function selectTab(tab) {
    if (!tabNames[tab]) return;
    clearInterval(state.screenTimer); state.screenTimer = null;
    state.tab = tab; document.querySelectorAll('.nav-item').forEach((n) => n.classList.toggle('active', n.dataset.tab === tab));
    $('#page-title').textContent = tabNames[tab]; $('#eyebrow').textContent = `ANDROID DEVICE / ${tab.toUpperCase()}`;
    void renderTab();
  }
  async function refreshDevices() {
    try {
      const devices = await invokeOrFail('adb_devices'); state.devices = devices;
      const online = devices.filter((d) => d.state === 'device');
      const picker = $('#device-select');
      if (!online.some((d) => d.serial === state.serial)) state.serial = online[0]?.serial ?? '';
      picker.innerHTML = online.length ? online.map((d) => `<option value="${escapeHtml(d.serial)}" ${d.serial === state.serial ? 'selected' : ''}>${escapeHtml(d.model || d.serial)}</option>`).join('') : '<option value="">No authorized device</option>';
      state.device = online.find((d) => d.serial === state.serial) || null;
      setConnection(Boolean(state.device), state.device ? `Connected · ${state.device.state}` : 'No authorized ADB device');
      $('#device-model').textContent = state.device?.model || 'No device selected';
      if (!state.device) { content.innerHTML = `<div class="card empty"><div class="empty-icon">▣</div><b>No Android device connected</b><span>Connect a device, enable USB debugging, and approve the computer’s RSA prompt.</span><button class="button tonal" id="retry-devices">Check again</button></div>`; return; }
      if (state.telemetry?.serial !== state.serial) state.telemetry = null;
      if (!state.telemetry) await refreshTelemetry();
    } catch (error) { setConnection(false, 'ADB unavailable'); content.innerHTML = `<div class="card empty"><div class="empty-icon">⌁</div><b>Could not reach ADB</b><span>${escapeHtml(error)}</span><button class="button tonal" id="retry-devices">Retry</button></div>`; }
  }
  async function refreshTelemetry() {
    if (!state.serial) return;
    try { state.telemetry = await invokeOrFail('adb_telemetry', { serial: state.serial }); state.telemetry.serial = state.serial; state.lastTelemetry = Date.now(); await renderTab(); }
    catch (e) { toast(String(e), true); }
  }
  async function loadData(section, path) {
    if (!state.serial) return [];
    try { state.data = await invokeOrFail('adb_data', { serial: state.serial, section, path: path ?? null }); return state.data; }
    catch (error) { state.data = []; toast(String(error), true); return []; }
  }
  async function action(action, value = '', extra = '') {
    try { const result = await invokeOrFail('adb_action', { serial: state.serial, action, value: value || null, extra: extra || null }); if (result && !['shell', 'clipboard_get'].includes(action)) toast(result); return result; }
    catch (error) { toast(String(error), true); throw error; }
  }
  const empty = (title, detail) => `<div class="empty"><div class="empty-icon">◇</div><b>${escapeHtml(title)}</b><span>${escapeHtml(detail)}</span></div>`;
  const card = (title, body, tools = '') => `<section class="card"><div class="card-head"><h2>${title}</h2><div>${tools}</div></div>${body}</section>`;
  const metric = (label, value, note, pct) => `<article class="card"><div class="metric-label">${label}</div><div class="metric-value">${value}</div><div class="metric-note">${note}</div>${pct == null ? '' : `<div class="progress"><i style="width:${Math.max(0, Math.min(100, pct))}%"></i></div>`}</article>`;
  function overview() {
    const t = state.telemetry || {}, battery = t.battery_pct;
    const used = t.storage_internal_total_gb && t.storage_internal_free_gb != null ? Math.max(0, 100 * (1 - t.storage_internal_free_gb / t.storage_internal_total_gb)) : null;
    return `<section class="card hero"><div><span class="badge">● ${escapeHtml(state.device?.state || 'device')}</span><h2>${escapeHtml(t.model || state.device?.model || state.serial)}</h2><div class="subtle">${escapeHtml(t.manufacturer || 'Android device')} · ${escapeHtml(state.serial)} · Android ${escapeHtml(val(t.android_version))}</div></div><div class="hero-actions"><button class="button tonal" data-tab="screen">▣ Screen share</button><button class="button primary" data-action="scrcpy">↗ Open scrcpy</button><button class="button" data-action="reboot" data-value="system">↻ Restart</button></div></section>
      <div class="grid metrics">${metric('Battery', val(battery, '%'), val(t.battery_status), battery)}${metric('Internal storage', t.storage_internal_total_gb != null ? `${(t.storage_internal_total_gb - (t.storage_internal_free_gb || 0)).toFixed(1)} GB` : 'Unavailable', t.storage_internal_total_gb != null ? `${(t.storage_internal_free_gb ?? 0).toFixed(1)} GB free` : 'Device data unavailable', used)}${metric('Display', escapeHtml(val(t.screen_res)), escapeHtml(val(t.screen_dpi)), null)}${metric('Uptime', t.uptime_secs ? `${Math.floor(t.uptime_secs / 3600)}h ${Math.floor(t.uptime_secs % 3600 / 60)}m` : 'Unavailable', `${escapeHtml(val(t.cpu_abi))} · API ${escapeHtml(val(t.api_level))}`, null)}</div>
      <div class="grid two">${card('Device information', `<div class="kv"><div><small>Brand</small><b>${escapeHtml(val(t.brand))}</b></div><div><small>Build</small><b>${escapeHtml(val(t.build_id))}</b></div><div><small>Security patch</small><b>${escapeHtml(val(t.security_patch))}</b></div><div><small>SELinux</small><b>${escapeHtml(val(t.selinux))} · ${t.is_rooted ? 'Rooted' : 'Unrooted'}</b></div></div>`)}${card('ADB connection', `<div class="kv"><div><small>Transport</small><b>${state.serial.includes(':') ? 'Wireless TCP/IP' : 'USB debugging'}</b></div><div><small>Product</small><b>${escapeHtml(val(state.device?.product))}</b></div><div><small>Device ID</small><b class="mono">${escapeHtml(state.serial)}</b></div><div><small>Refresh cadence</small><b>On demand · low overhead</b></div></div>`)}</div>`;
  }
  async function renderTab() {
    if (!state.device) return;
    if (state.tab === 'overview') { content.innerHTML = overview(); return; }
    if (tabKinds[state.tab]) await loadData(tabKinds[state.tab], state.path);
    if (!state.device) return;
    switch (state.tab) {
      case 'screen': return renderScreen();
      case 'messages': return renderMessages();
      case 'calls': return renderCalls();
      case 'photos': return renderPhotos();
      case 'notifications': return renderNotifications();
      case 'storage': return renderStorage();
      case 'apps': return renderApps();
      case 'clipboard': return renderClipboard();
      case 'terminal': return renderTerminal();
      case 'wireless': return renderWireless();
      case 'firmware': return renderFirmware();
    }
  }
  function renderScreen() {
    content.innerHTML = `<div class="screen-layout"><section class="card screen-stage"><div class="screen-phone" id="screen-image">${empty('Screen capture ready', 'Use Refresh to fetch the current device screen.')}</div></section><div class="stack"><section class="card"><div class="card-head"><h2>Screen controls</h2></div><div class="toolbar"><button class="button primary" id="screen-refresh">↻ Refresh screen</button><button class="button tonal" data-action="scrcpy">Open scrcpy</button></div><div class="screen-controls"><button class="button" data-action="key_back">← Back</button><button class="button" data-action="key_home">⌂ Home</button><button class="button" data-action="key_recents">▢ Recent</button><button class="button" data-action="key_volume_down">Vol −</button><button class="button" data-action="key_volume_up">Vol +</button><button class="button" data-action="key_power">Power</button></div><div class="form-row" style="margin-top:12px"><input class="input" id="screen-text" placeholder="Type text on device…"><button class="button" id="send-screen-text">Send</button></div></section><section class="card"><h3>Mirror options</h3><p class="subtle">For smooth interactive control, scrcpy runs in its own optimized window. Static screen preview refreshes every 3 seconds while this tab is active.</p></section></div></div>`;
    const take = async () => { const target = $('#screen-image'); if (!target || !state.serial) return; try { const src = await invokeOrFail('adb_screenshot', { serial: state.serial }); target.innerHTML = `<img alt="Current Android screen" src="${src}">`; } catch (e) { target.innerHTML = `<div class="muted">${escapeHtml(e)}</div>`; } };
    $('#screen-refresh').onclick = take; void take(); state.screenTimer = setInterval(take, 3000);
    $('#send-screen-text').onclick = async () => { const text = $('#screen-text').value; if (text) await action('send_text', text); };
  }
  function renderMessages() {
    const rows = state.data || [];
    content.innerHTML = `<div class="grid two"><section class="card"><div class="card-head"><h2>Messages</h2><button class="button" data-refresh>↻ Refresh</button></div><div class="stack">${rows.length ? rows.map(m => `<article class="list-item"><div class="list-item-top"><div class="list-title">${escapeHtml(m.contact || m.address || 'Unknown contact')}</div><span class="list-meta">${escapeHtml(m.time)}</span></div><p>${escapeHtml(m.text)}</p></article>`).join('') : empty('No messages available', 'SMS data is returned by the connected device when access is available.')}</div></section><section class="card"><div class="card-head"><h2>Send an SMS</h2></div><div class="stack"><input id="sms-number" class="input" placeholder="Phone number"><textarea id="sms-body" placeholder="Write your message…"></textarea><button class="button primary" id="sms-send">Send message</button><div class="subtle">The phone and carrier handle delivery. Charges may apply.</div></div></section></div>`;
    $('#sms-send').onclick = async () => { const n = $('#sms-number').value.trim(), b = $('#sms-body').value.trim(); if (!n || !b) return toast('Enter a number and message.', true); await action('send_sms', n, b); };
  }
  function renderCalls() {
    const rows = state.data || [];
    content.innerHTML = `<div class="grid two"><section class="card"><div class="card-head"><h2>Recent calls</h2><button class="button" data-refresh>↻ Refresh</button></div><div class="toolbar"><input id="call-filter" class="input" placeholder="Search calls"></div><div id="call-list" class="stack">${rows.length ? rows.map(c => `<article class="list-item call-row"><div class="list-item-top"><div><div class="list-title">${escapeHtml(c.name || c.number || 'Unknown caller')}</div><div class="list-meta">${escapeHtml(c.kind)} · ${escapeHtml(c.duration)}</div></div><span class="list-meta">${escapeHtml(c.time)}</span></div><div class="toolbar" style="margin:10px 0 0"><button class="button tonal" data-action="call" data-value="${escapeHtml(c.number)}">Call back</button></div></article>`).join('') : empty('No call history', 'Call log records will appear here when available.')}</div></section><section class="card"><div class="card-head"><h2>Keypad</h2><span class="badge">DEVICE CALL</span></div><input id="dial-number" class="input" style="font-size:18px;margin-bottom:14px" placeholder="Phone number" inputmode="tel"><div class="keypad">${['1','2','3','4','5','6','7','8','9','*','0','#'].map(k => `<button class="button key" data-key="${k}">${k}</button>`).join('')}</div><button class="button success" style="width:100%;margin-top:12px" id="dial-call">☎ Start call</button><button class="button danger" style="width:100%;margin-top:8px" data-action="hangup">End call</button></section></div>`;
    $('#call-filter').oninput = (e) => document.querySelectorAll('.call-row').forEach(row => row.hidden = !row.textContent.toLowerCase().includes(e.target.value.toLowerCase()));
    document.querySelectorAll('[data-key]').forEach(k => k.onclick = () => { $('#dial-number').value += k.dataset.key; });
    $('#dial-call').onclick = () => { const n = $('#dial-number').value.trim(); if (n) void action('call', n); else toast('Enter a phone number.', true); };
  }
  function renderPhotos() {
    const rows = state.data || [];
    content.innerHTML = `<section class="card"><div class="card-head"><div><h2>Camera roll</h2><div class="subtle" style="margin-top:5px">${rows.length} items · Preview downloads one image only when requested</div></div><button class="button" data-refresh>↻ Refresh</button></div>${rows.length ? `<div class="photo-grid">${rows.map((p,i) => `<article class="photo-card"><div id="photo-preview-${i}" class="photo-preview">▧</div><strong title="${escapeHtml(p.name)}">${escapeHtml(p.name)}</strong><div class="list-meta">${escapeHtml(p.size)} · ${escapeHtml(p.date)}</div><div class="photo-actions"><button class="button" data-view-photo="${i}">Preview</button><button class="button success" data-action="download_photo" data-value="${escapeHtml(p.path)}">Save</button><button class="button danger" data-action="delete_photo" data-value="${escapeHtml(p.path)}">Delete</button></div></article>`).join('')}</div>` : empty('No photos found', 'No images were returned from the device camera folders.')}</section>`;
    document.querySelectorAll('[data-view-photo]').forEach(btn => btn.onclick = async () => { const p = rows[Number(btn.dataset.viewPhoto)], slot = $(`#photo-preview-${btn.dataset.viewPhoto}`); btn.disabled = true; try { const src = await invokeOrFail('adb_photo_preview', { serial: state.serial, path: p.path }); slot.innerHTML = `<img alt="${escapeHtml(p.name)}" src="${src}">`; } catch (e) { toast(String(e), true); } finally { btn.disabled = false; } });
  }
  function renderNotifications() {
    const rows = state.data || [];
    content.innerHTML = `<section class="card"><div class="card-head"><div><h2>Notifications</h2><div class="subtle" style="margin-top:5px">${rows.length} active device notifications</div></div><button class="button" data-refresh>↻ Refresh</button></div><div class="stack">${rows.length ? rows.map(n => `<article class="list-item"><div class="list-item-top"><div><div class="list-title">${escapeHtml(n.title || n.app || n.package)}</div><div class="list-meta">${escapeHtml(n.app)} · ${escapeHtml(n.time)}</div></div><span class="chip">${escapeHtml(n.package)}</span></div><p>${escapeHtml(n.text)}</p></article>`).join('') : empty('No active notifications', 'The device did not report any currently posted notifications.')}</div></section>`;
  }
  function renderStorage() {
    const rows = state.data || [], t = state.telemetry || {};
    content.innerHTML = `<div class="grid metrics">${metric('Internal used', t.storage_internal_total_gb != null ? `${(t.storage_internal_total_gb-(t.storage_internal_free_gb??0)).toFixed(1)} GB` : 'Unavailable', t.storage_internal_total_gb != null ? `${(t.storage_internal_free_gb??0).toFixed(1)} GB free of ${t.storage_internal_total_gb.toFixed(1)} GB` : 'No storage telemetry', t.storage_internal_total_gb != null ? 100*(1-t.storage_internal_free_gb/t.storage_internal_total_gb) : null)}${metric('Shared storage', t.storage_sdcard_total_gb != null ? `${(t.storage_sdcard_total_gb-(t.storage_sdcard_free_gb??0)).toFixed(1)} GB` : 'Unavailable', t.storage_sdcard_total_gb != null ? `${(t.storage_sdcard_free_gb??0).toFixed(1)} GB free` : 'No shared storage telemetry', null)}</div><section class="card"><div class="card-head"><div><h2>File browser</h2><div class="subtle mono" style="margin-top:5px">${escapeHtml(state.path)}</div></div><button class="button" data-refresh>↻ Refresh</button></div><form id="path-form" class="toolbar"><input id="file-path" class="input" value="${escapeHtml(state.path)}"><button class="button primary">Go to path</button><button type="button" class="button" id="file-up">Parent folder</button></form><div class="table-wrap"><table class="table"><thead><tr><th>Name</th><th>Type</th><th>Size</th><th>Modified</th><th></th></tr></thead><tbody>${rows.length ? rows.map((f,i) => `<tr><td><button class="file-open" data-file-index="${i}">${f.directory?'▱':'·'} ${escapeHtml(f.name)}</button></td><td>${f.directory?'Folder':'File'}</td><td>${f.bytes ? `${(f.bytes/1024).toFixed(1)} KB` : '—'}</td><td>${escapeHtml(f.modified)}</td><td><button class="button danger" data-action="delete_file" data-value="${escapeHtml(state.path.replace(/\/$/, '')+'/'+f.name)}">Delete</button></td></tr>`).join('') : `<tr><td colspan="5">${empty('Folder is empty', 'Try another folder path.')}</td></tr>`}</tbody></table></div></section>`;
    $('#path-form').onsubmit = (e) => { e.preventDefault(); state.path = $('#file-path').value.trim() || '/sdcard'; void renderTab(); };
    $('#file-up').onclick = () => { state.path = state.path.replace(/\/$/, '').split('/').slice(0,-1).join('/') || '/'; void renderTab(); };
    document.querySelectorAll('[data-file-index]').forEach(b => b.onclick = () => { const f = rows[Number(b.dataset.fileIndex)]; if (f.directory) { state.path = `${state.path.replace(/\/$/, '')}/${f.name}`; void renderTab(); } });
  }
  function renderApps() {
    const rows = state.data || [];
    content.innerHTML = `<section class="card"><div class="card-head"><div><h2>Installed apps</h2><div class="subtle" style="margin-top:5px">${rows.length} user apps</div></div><button class="button" data-refresh>↻ Refresh</button></div><div class="toolbar"><input id="app-filter" class="input" placeholder="Filter package name"></div><div class="table-wrap"><table class="table"><thead><tr><th>Package</th><th>Actions</th></tr></thead><tbody>${rows.map((a,i)=>`<tr class="app-row"><td class="mono">${escapeHtml(a.name)}</td><td><button class="button tonal" data-action="launch_app" data-value="${escapeHtml(a.name)}">Open</button> <button class="button danger" data-action="stop_app" data-value="${escapeHtml(a.name)}">Force stop</button></td></tr>`).join('') || `<tr><td colspan="2">${empty('No apps returned', 'Could not read the user app list.')}</td></tr>`}</tbody></table></div></section>`;
    $('#app-filter').oninput = e => document.querySelectorAll('.app-row').forEach(r => r.hidden = !r.textContent.toLowerCase().includes(e.target.value.toLowerCase()));
  }
  async function renderClipboard() {
    content.innerHTML = `<section class="card"><div class="card-head"><div><h2>Clipboard</h2><div class="subtle" style="margin-top:5px">Read or send text to the Android clipboard</div></div></div><div class="stack"><textarea id="clipboard-text" placeholder="Clipboard text…"></textarea><div class="toolbar"><button id="clipboard-read" class="button">Read device clipboard</button><button id="clipboard-send" class="button primary">Send to device</button></div></div></section>`;
    $('#clipboard-read').onclick = async () => { try { $('#clipboard-text').value = await action('clipboard_get'); } catch {} };
    $('#clipboard-send').onclick = async () => { await action('clipboard_set', $('#clipboard-text').value); };
  }
  function renderTerminal() {
    content.innerHTML = `<section class="card"><div class="card-head"><div><h2>ADB shell</h2><div class="subtle" style="margin-top:5px">Commands run on the selected device over the local ADB bridge.</div></div></div><form id="shell-form" class="form-row"><input id="shell-command" class="input mono" placeholder="getprop ro.build.version.release" autocomplete="off"><button class="button primary">Run</button></form><pre id="shell-output" class="console" style="margin-top:14px">Ready. Output appears here.</pre></section>`;
    $('#shell-form').onsubmit = async e => { e.preventDefault(); const cmd=$('#shell-command').value.trim(); if(!cmd)return; const out=$('#shell-output'); out.textContent=`$ ${cmd}\nRunning…`; try { out.textContent=`$ ${cmd}\n${await action('shell',cmd) || '(completed)'}`; } catch(err) { out.textContent=`$ ${cmd}\n${err}`; } };
  }
  function renderWireless() {
    const rows=state.data||[];
    content.innerHTML=`<div class="grid two"><section class="card"><div class="card-head"><div><h2>Wireless debugging</h2><div class="subtle" style="margin-top:5px">Switch the selected USB device to ADB over TCP/IP.</div></div></div><div class="stack"><div class="notice">Enabling TCP/IP debugging exposes ADB on the local network until you switch back to USB or reboot. Use this only on a trusted network.</div><div class="toolbar"><button class="button primary" data-action="wireless_on">Enable TCP/IP · 5555</button><button class="button" data-action="wireless_off">Switch back to USB</button><button id="endpoint-refresh" class="button">Refresh addresses</button></div></div></section><section class="card"><div class="card-head"><h2>Device network addresses</h2></div><div class="stack">${rows.length?rows.map(e=>`<article class="list-item"><div class="list-title">${escapeHtml(e.label)}</div><div class="mono muted" style="margin-top:5px">${escapeHtml(e.ip)} · ${escapeHtml(e.interface)}</div></article>`).join(''):empty('No network addresses', 'Connect the device to a network and refresh addresses.')}</div></section></div>`;
    $('#endpoint-refresh').onclick=()=>void renderTab();
  }
  function renderFirmware() {
    content.innerHTML=`<div class="grid two"><section class="card"><div class="card-head"><h2>Power and navigation</h2></div><div class="toolbar"><button class="button" data-action="key_power">Power / Wake</button><button class="button" data-action="key_back">Back</button><button class="button" data-action="key_home">Home</button><button class="button" data-action="key_recents">Recents</button><button class="button" data-action="key_volume_down">Volume −</button><button class="button" data-action="key_volume_up">Volume +</button></div></section><section class="card"><div class="card-head"><h2>Restart device</h2></div><div class="stack"><div class="notice">Recovery and bootloader restart the device outside Android. The device may disconnect until it boots again.</div><div class="toolbar"><button class="button danger" data-action="reboot" data-value="system">System</button><button class="button" data-action="reboot" data-value="recovery">Recovery</button><button class="button" data-action="reboot" data-value="bootloader">Bootloader</button><button class="button" data-action="reboot" data-value="fastboot">Fastboot</button></div></div></section></div>`;
  }
  async function refreshCurrent() { if (state.tab === 'overview' || state.tab === 'storage') await refreshTelemetry(); else await renderTab(); }

  document.addEventListener('click', async (event) => {
    const tab = event.target.closest('[data-tab]'); if (tab) { selectTab(tab.dataset.tab); return; }
    if (event.target.closest('[data-refresh]') || event.target.closest('#refresh')) { await refreshCurrent(); return; }
    const button=event.target.closest('[data-action]'); if (button) {
      if (['delete_file','delete_photo'].includes(button.dataset.action) && !window.confirm('Delete this item from the Android device?')) return;
      if (button.dataset.action === 'reboot' && button.dataset.value !== 'system' && !window.confirm(`Restart device to ${button.dataset.value}?`)) return;
      button.disabled=true;
      try { await action(button.dataset.action,button.dataset.value || ''); if (['delete_photo','delete_file'].includes(button.dataset.action)) await renderTab(); }
      catch {} finally { button.disabled=false; }
    }
    if(event.target.closest('#retry-devices')) await refreshDevices();
  });
  $('#device-select').addEventListener('change', async e => { state.serial=e.target.value; state.telemetry=null; await refreshDevices(); });
  $('#refresh').addEventListener('click', () => void refreshCurrent());
  document.addEventListener('visibilitychange', () => { if(document.hidden){clearInterval(state.screenTimer);state.screenTimer=null;} else if(state.tab==='screen'&&state.serial) selectTab('screen'); });
  (async () => {
    if (!invoke) { content.innerHTML=`<div class="card empty"><b>Tauri runtime is unavailable</b><span>Launch the Android cockpit from Server Launcher.</span></div>`; return; }
    try { state.serial=await invoke('adb_initial_serial') || ''; await refreshDevices(); } catch(e) { toast(String(e),true); }
    setInterval(async () => { if(document.hidden)return; const old=state.serial; await refreshDevices(); if(state.serial===old && state.tab!=='screen' && Date.now()-state.lastTelemetry>30000) await refreshTelemetry(); }, 7000);
  })();
})();
