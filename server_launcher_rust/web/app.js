(() => {
  const invoke = window.__TAURI__?.core?.invoke;
  const $ = (selector) => document.querySelector(selector);
  const content = $('#content');
  const state = { serial: '', device: null, devices: [], tab: 'overview', telemetry: null, data: [], sensorsView: 'overview', sensorHistory: [], sensorTimer: null, sensorBusy: false, sensorFilter: '', sensorSort: 'cpu', path: '/sdcard', screenTimer: null, toastTimer: null, lastTelemetry: 0 };
  const tabNames = { overview: 'Overview', screen: 'Screen share', messages: 'Messages', calls: 'Calls', photos: 'Photos', notifications: 'Notifications', sensors: 'Sensors', storage: 'Storage', apps: 'Apps', clipboard: 'Clipboard', terminal: 'Terminal', wireless: 'Wireless debugging', firmware: 'Device controls' };
  const tabKinds = { messages: 'messages', calls: 'calls', photos: 'photos', notifications: 'notifications', sensors: 'sensors', apps: 'apps', storage: 'files', wireless: 'endpoints' };
  const icon = (code) => `<span class="icon" aria-hidden="true">&#x${code};</span>`;
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
    clearTimeout(state.sensorTimer); state.sensorTimer = null;
    $('#page-title').textContent = tabNames[tab]; $('#eyebrow').textContent = `ANDROID DEVICE / ${tab.toUpperCase()}`;
    void renderTab();
  }
  async function refreshDevices() {
    try {
      const devices = await invokeOrFail('adb_devices'); state.devices = devices;
      const online = devices.filter((d) => d.state === 'device');
      const picker = $('#device-select');
      if (!online.some((d) => d.serial === state.serial)) state.serial = online[0]?.serial ?? '';
      if (state.sensorHistory.length && state.sensorHistorySerial !== state.serial) { state.sensorHistory = []; state.sensorHistorySerial = state.serial; }
      picker.innerHTML = online.length ? online.map((d) => `<option value="${escapeHtml(d.serial)}" ${d.serial === state.serial ? 'selected' : ''}>${escapeHtml(d.model || d.serial)}</option>`).join('') : '<option value="">No authorized device</option>';
      state.device = online.find((d) => d.serial === state.serial) || null;
      setConnection(Boolean(state.device), state.device ? `Connected · ${state.device.state}` : 'No authorized ADB device');
      $('#device-model').textContent = state.device?.model || 'No device selected';
      if (!state.device) { content.innerHTML = `<div class="card empty"><div class="empty-icon">${icon("e163")}</div><b>No Android device connected</b><span>Connect a device, enable USB debugging, and approve the computer’s RSA prompt.</span><button class="button tonal" id="retry-devices">Check again</button></div>`; return; }
      if (state.telemetry?.serial !== state.serial) state.telemetry = null;
      if (!state.telemetry) await refreshTelemetry();
    } catch (error) { setConnection(false, 'ADB unavailable'); content.innerHTML = `<div class="card empty"><div class="empty-icon">${icon("e1ae")}</div><b>Could not reach ADB</b><span>${escapeHtml(error)}</span><button class="button tonal" id="retry-devices">Retry</button></div>`; }
  }
  async function refreshTelemetry() {
    if (!state.serial) return;
    try { state.telemetry = await invokeOrFail('adb_telemetry', { serial: state.serial }); state.telemetry.serial = state.serial; state.lastTelemetry = Date.now(); await renderTab(); }
    catch (e) { toast(String(e), true); }
  }
  async function loadData(section, path) {
    if (!state.serial) return [];
    try { state.data = await invokeOrFail('adb_data', { serial: state.serial, section, path: path ?? null }); if (section === 'sensors') recordSensorSample(state.data); return state.data; }
    catch (error) { state.data = []; toast(String(error), true); return []; }
  }
  async function action(action, value = '', extra = '') {
    try { const result = await invokeOrFail('adb_action', { serial: state.serial, action, value: value || null, extra: extra || null }); if (result && !['shell', 'clipboard_get'].includes(action)) toast(result); return result; }
    catch (error) { toast(String(error), true); throw error; }
  }
  const empty = (title, detail) => `<div class="empty"><div class="empty-icon">${icon("e0f9")}</div><b>${escapeHtml(title)}</b><span>${escapeHtml(detail)}</span></div>`;
  const card = (title, body, tools = '') => `<section class="card"><div class="card-head"><h2>${title}</h2><div>${tools}</div></div>${body}</section>`;
  const metric = (label, value, note, pct) => `<article class="card"><div class="metric-label">${label}</div><div class="metric-value">${value}</div><div class="metric-note">${note}</div>${pct == null ? '' : `<div class="progress"><i style="width:${Math.max(0, Math.min(100, pct))}%"></i></div>`}</article>`;
  function overview() {
    const t = state.telemetry || {}, battery = t.battery_pct;
    const used = t.storage_internal_total_gb && t.storage_internal_free_gb != null ? Math.max(0, 100 * (1 - t.storage_internal_free_gb / t.storage_internal_total_gb)) : null;
    return `<section class="card hero"><div><span class="badge">● ${escapeHtml(state.device?.state || 'device')}</span><h2>${escapeHtml(t.model || state.device?.model || state.serial)}</h2><div class="subtle">${escapeHtml(t.manufacturer || 'Android device')} · ${escapeHtml(state.serial)} · Android ${escapeHtml(val(t.android_version))}</div></div><div class="hero-actions"><button class="button tonal" data-tab="screen">${icon("e14f")} Screen share</button><button class="button primary" data-action="scrcpy">${icon("e0b9")} Open scrcpy</button><button class="button" data-action="reboot" data-value="system">${icon("e145")} Restart</button></div></section>
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
      case 'sensors': return renderSensors();
      case 'storage': return renderStorage();
      case 'apps': return renderApps();
      case 'clipboard': return renderClipboard();
      case 'terminal': return renderTerminal();
      case 'wireless': return renderWireless();
      case 'firmware': return renderFirmware();
    }
  }
  function renderScreen() {
    content.innerHTML = `<div class="screen-layout"><section class="card screen-stage"><div class="screen-phone" id="screen-image">${empty('Screen capture ready', 'Use Refresh to fetch the current device screen.')}</div></section><div class="stack"><section class="card"><div class="card-head"><h2>Screen controls</h2></div><div class="toolbar"><button class="button primary" id="screen-refresh">${icon("e145")} Refresh screen</button><button class="button tonal" data-action="scrcpy">Open scrcpy</button></div><div class="screen-controls"><button class="button" data-action="key_back">${icon("e048")} Back</button><button class="button" data-action="key_home">${icon("e101")} Home</button><button class="button" data-action="key_recents">${icon("e101")} Recent</button><button class="button" data-action="key_volume_down">Vol −</button><button class="button" data-action="key_volume_up">Vol +</button><button class="button" data-action="key_power">Power</button></div><div class="form-row" style="margin-top:12px"><input class="input" id="screen-text" placeholder="Type text on device…"><button class="button" id="send-screen-text">Send</button></div></section><section class="card"><h3>Mirror options</h3><p class="subtle">For smooth interactive control, scrcpy runs in its own optimized window. Static screen preview refreshes every 3 seconds while this tab is active.</p></section></div></div>`;
    const take = async () => { const target = $('#screen-image'); if (!target || !state.serial) return; try { const src = await invokeOrFail('adb_screenshot', { serial: state.serial }); target.innerHTML = `<img alt="Current Android screen" src="${src}">`; } catch (e) { target.innerHTML = `<div class="muted">${escapeHtml(e)}</div>`; } };
    $('#screen-refresh').onclick = take; void take(); state.screenTimer = setInterval(take, 3000);
    $('#send-screen-text').onclick = async () => { const text = $('#screen-text').value; if (text) await action('send_text', text); };
  }
  function renderMessages() {
    const rows = state.data || [];
    content.innerHTML = `<div class="grid two"><section class="card"><div class="card-head"><h2>Messages</h2><button class="button" data-refresh>${icon("e145")} Refresh</button></div><div class="stack">${rows.length ? rows.map(m => `<article class="list-item"><div class="list-item-top"><div class="list-title">${escapeHtml(m.contact || m.address || 'Unknown contact')}</div><span class="list-meta">${escapeHtml(m.time)}</span></div><p>${escapeHtml(m.text)}</p></article>`).join('') : empty('No messages available', 'SMS data is returned by the connected device when access is available.')}</div></section><section class="card"><div class="card-head"><h2>Send an SMS</h2></div><div class="stack"><input id="sms-number" class="input" placeholder="Phone number"><textarea id="sms-body" placeholder="Write your message…"></textarea><button class="button primary" id="sms-send">Send message</button><div class="subtle">The phone and carrier handle delivery. Charges may apply.</div></div></section></div>`;
    $('#sms-send').onclick = async () => { const n = $('#sms-number').value.trim(), b = $('#sms-body').value.trim(); if (!n || !b) return toast('Enter a number and message.', true); await action('send_sms', n, b); };
  }
  function renderCalls() {
    const rows = state.data || [];
    content.innerHTML = `<div class="grid two"><section class="card"><div class="card-head"><h2>Recent calls</h2><button class="button" data-refresh>${icon("e145")} Refresh</button></div><div class="toolbar"><input id="call-filter" class="input" placeholder="Search calls"></div><div id="call-list" class="stack">${rows.length ? rows.map(c => `<article class="list-item call-row"><div class="list-item-top"><div><div class="list-title">${escapeHtml(c.name || c.number || 'Unknown caller')}</div><div class="list-meta">${escapeHtml(c.kind)} · ${escapeHtml(c.duration)}</div></div><span class="list-meta">${escapeHtml(c.time)}</span></div><div class="toolbar" style="margin:10px 0 0"><button class="button tonal" data-action="call" data-value="${escapeHtml(c.number)}">Call back</button></div></article>`).join('') : empty('No call history', 'Call log records will appear here when available.')}</div></section><section class="card"><div class="card-head"><h2>Keypad</h2><span class="badge">DEVICE CALL</span></div><input id="dial-number" class="input" style="font-size:18px;margin-bottom:14px" placeholder="Phone number" inputmode="tel"><div class="keypad">${['1','2','3','4','5','6','7','8','9','*','0','#'].map(k => `<button class="button key" data-key="${k}">${k}</button>`).join('')}</div><button class="button success" style="width:100%;margin-top:12px" id="dial-call">${icon("e135")} Start call</button><button class="button danger" style="width:100%;margin-top:8px" data-action="hangup">End call</button></section></div>`;
    $('#call-filter').oninput = (e) => document.querySelectorAll('.call-row').forEach(row => row.hidden = !row.textContent.toLowerCase().includes(e.target.value.toLowerCase()));
    document.querySelectorAll('[data-key]').forEach(k => k.onclick = () => { $('#dial-number').value += k.dataset.key; });
    $('#dial-call').onclick = () => { const n = $('#dial-number').value.trim(); if (n) void action('call', n); else toast('Enter a phone number.', true); };
  }
  function renderPhotos() {
    const rows = state.data || [];
    content.innerHTML = `<section class="card"><div class="card-head"><div><h2>Camera roll</h2><div class="subtle" style="margin-top:5px">${rows.length} items · Preview downloads one image only when requested</div></div><button class="button" data-refresh>${icon("e145")} Refresh</button></div>${rows.length ? `<div class="photo-grid">${rows.map((p,i) => `<article class="photo-card"><div id="photo-preview-${i}" class="photo-preview">${icon("e0f7")}</div><strong title="${escapeHtml(p.name)}">${escapeHtml(p.name)}</strong><div class="list-meta">${escapeHtml(p.size)} · ${escapeHtml(p.date)}</div><div class="photo-actions"><button class="button" data-view-photo="${i}">Preview</button><button class="button success" data-action="download_photo" data-value="${escapeHtml(p.path)}">Save</button><button class="button danger" data-action="delete_photo" data-value="${escapeHtml(p.path)}">Delete</button></div></article>`).join('')}</div>` : empty('No photos found', 'No images were returned from the device camera folders.')}</section>`;
    document.querySelectorAll('[data-view-photo]').forEach(btn => btn.onclick = async () => { const p = rows[Number(btn.dataset.viewPhoto)], slot = $(`#photo-preview-${btn.dataset.viewPhoto}`); btn.disabled = true; try { const src = await invokeOrFail('adb_photo_preview', { serial: state.serial, path: p.path }); slot.innerHTML = `<img alt="${escapeHtml(p.name)}" src="${src}">`; } catch (e) { toast(String(e), true); } finally { btn.disabled = false; } });
  }
  function renderNotifications() {
    const rows = state.data || [];
    content.innerHTML = `<section class="card"><div class="card-head"><div><h2>Notifications</h2><div class="subtle" style="margin-top:5px">${rows.length} active device notifications</div></div><button class="button" data-refresh>${icon("e145")} Refresh</button></div><div class="stack">${rows.length ? rows.map(n => `<article class="list-item"><div class="list-item-top"><div><div class="list-title">${escapeHtml(n.title || n.app || n.package)}</div><div class="list-meta">${escapeHtml(n.app)} · ${escapeHtml(n.time)}</div></div><span class="chip">${escapeHtml(n.package)}</span></div><p>${escapeHtml(n.text)}</p></article>`).join('') : empty('No active notifications', 'The device did not report any currently posted notifications.')}</div></section>`;
  }
  function renderStorage() {
    const rows = state.data || [], t = state.telemetry || {};
    content.innerHTML = `<div class="grid metrics">${metric('Internal used', t.storage_internal_total_gb != null ? `${(t.storage_internal_total_gb-(t.storage_internal_free_gb??0)).toFixed(1)} GB` : 'Unavailable', t.storage_internal_total_gb != null ? `${(t.storage_internal_free_gb??0).toFixed(1)} GB free of ${t.storage_internal_total_gb.toFixed(1)} GB` : 'No storage telemetry', t.storage_internal_total_gb != null ? 100*(1-t.storage_internal_free_gb/t.storage_internal_total_gb) : null)}${metric('Shared storage', t.storage_sdcard_total_gb != null ? `${(t.storage_sdcard_total_gb-(t.storage_sdcard_free_gb??0)).toFixed(1)} GB` : 'Unavailable', t.storage_sdcard_total_gb != null ? `${(t.storage_sdcard_free_gb??0).toFixed(1)} GB free` : 'No shared storage telemetry', null)}</div><section class="card"><div class="card-head"><div><h2>File browser</h2><div class="subtle mono" style="margin-top:5px">${escapeHtml(state.path)}</div></div><button class="button" data-refresh>${icon("e145")} Refresh</button></div><form id="path-form" class="toolbar"><input id="file-path" class="input" value="${escapeHtml(state.path)}"><button class="button primary">Go to path</button><button type="button" class="button" id="file-up">Parent folder</button></form><div class="table-wrap"><table class="table"><thead><tr><th>Name</th><th>Type</th><th>Size</th><th>Modified</th><th></th></tr></thead><tbody>${rows.length ? rows.map((f,i) => `<tr><td><button class="file-open" data-file-index="${i}">${f.directory?icon("e0d7"):icon("e0cc")} ${escapeHtml(f.name)}</button></td><td>${f.directory?'Folder':'File'}</td><td>${f.bytes ? `${(f.bytes/1024).toFixed(1)} KB` : '—'}</td><td>${escapeHtml(f.modified)}</td><td><button class="button danger" data-action="delete_file" data-value="${escapeHtml(state.path.replace(/\/$/, '')+'/'+f.name)}">Delete</button></td></tr>`).join('') : `<tr><td colspan="5">${empty('Folder is empty', 'Try another folder path.')}</td></tr>`}</tbody></table></div></section>`;
    $('#path-form').onsubmit = (e) => { e.preventDefault(); state.path = $('#file-path').value.trim() || '/sdcard'; void renderTab(); };
    $('#file-up').onclick = () => { state.path = state.path.replace(/\/$/, '').split('/').slice(0,-1).join('/') || '/'; void renderTab(); };
    document.querySelectorAll('[data-file-index]').forEach(b => b.onclick = () => { const f = rows[Number(b.dataset.fileIndex)]; if (f.directory) { state.path = `${state.path.replace(/\/$/, '')}/${f.name}`; void renderTab(); } });
  }
  function renderApps() {
    const rows = state.data || [];
    content.innerHTML = `<section class="card"><div class="card-head"><div><h2>Installed apps</h2><div class="subtle" style="margin-top:5px">${rows.length} user apps</div></div><button class="button" data-refresh>${icon("e145")} Refresh</button></div><div class="toolbar"><input id="app-filter" class="input" placeholder="Filter package name"></div><div class="table-wrap"><table class="table"><thead><tr><th>Package</th><th>Actions</th></tr></thead><tbody>${rows.map((a,i)=>`<tr class="app-row"><td class="mono">${escapeHtml(a.name)}</td><td><button class="button tonal" data-action="launch_app" data-value="${escapeHtml(a.name)}">Open</button> <button class="button danger" data-action="stop_app" data-value="${escapeHtml(a.name)}">Force stop</button></td></tr>`).join('') || `<tr><td colspan="2">${empty('No apps returned', 'Could not read the user app list.')}</td></tr>`}</tbody></table></div></section>`;
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
  function recordSensorSample(s) {
    if (!s || typeof s !== 'object') return;
    state.sensorHistorySerial = state.serial;
    const previous = state.sensorHistory.at(-1), now = Date.now();
    const sum = (items, key) => (items || []).reduce((n, item) => n + (Number(item[key]) || 0), 0);
    const mem = s.mem_total_kb ? 100 * (s.mem_total_kb - (s.mem_available_kb || 0)) / s.mem_total_kb : null;
    const diskTotal = s.storage_internal_total_kb, diskFree = s.storage_internal_free_kb;
    const rx = sum(s.network_interfaces, 'rx_bytes'), tx = sum(s.network_interfaces, 'tx_bytes');
    const diskRead = sum(s.disk_counters, 'read_sectors'), diskWrite = sum(s.disk_counters, 'written_sectors');
    const seconds = previous ? Math.max(.1, (now - previous.at) / 1000) : 0;
    const rate = (current, old) => previous && current >= old ? (current - old) / seconds : null;
    const temps = s.temperatures || [];
    const cores = {};
    for (const c of s.cpu_counters || []) {
      if (c.name === 'cpu' || !previous) continue;
      const old = (previous.cpuCounters || {})[c.name];
      if (old && c.total > old.total && c.idle >= old.idle) cores[c.name] = 100 * (1 - (c.idle - old.idle) / (c.total - old.total));
    }
    state.sensorHistory.push({ at: now, cpu: s.cpu_usage_pct, ram: mem,
      swap: s.swap_total_kb ? 100 * (s.swap_total_kb - (s.swap_free_kb || 0)) / s.swap_total_kb : null,
      storage: diskTotal && diskFree != null ? 100 * (diskTotal - diskFree) / diskTotal : null,
      rx, tx, rxRate: rate(rx, previous?.rx || 0), txRate: rate(tx, previous?.tx || 0),
      diskReadRate: rate(diskRead, previous?.diskRead || 0) == null ? null : rate(diskRead, previous?.diskRead || 0) * 512,
      diskWriteRate: rate(diskWrite, previous?.diskWrite || 0) == null ? null : rate(diskWrite, previous?.diskWrite || 0) * 512,
      battery: s.battery_pct, temp: temps.length ? Math.max(...temps.map(x => Number(x.temp_c)).filter(Number.isFinite)) : s.battery_temp_c,
      cores, cpuCounters: Object.fromEntries((s.cpu_counters || []).map(c => [c.name, { total: c.total, idle: c.idle }])), diskRead, diskWrite });
    if (state.sensorHistory.length > 60) state.sensorHistory.shift();
  }
  function sensorChart(title, keys, labels, suffix = '%') {
    const rows = state.sensorHistory.filter(row => keys.some(key => Number.isFinite(row[key] ?? row.cores?.[key])));
    if (rows.length < 2) return `<section class="card sensor-chart"><div class="card-head"><h2>${title}</h2><span class="subtle">Collecting samples · ${rows.length}/2</span></div><div class="chart-empty">Charts appear after two live samples.</div></section>`;
    const colors = ['var(--accent)', 'var(--cyan)', 'var(--green)'], w = 600, h = 126, pad = 6;
    const values = rows.flatMap(row => keys.map(key => row[key] ?? row.cores?.[key])).filter(Number.isFinite);
    let min = suffix === ' °C' ? Math.floor(Math.min(...values) - 1) : Math.min(0, ...values), max = Math.max(...values); if (max === min) max = min + 1;
    const axis = value => {
      if (suffix.includes('B')) { const units=['B','KB','MB','GB','TB']; let n=value,i=0; while(Math.abs(n)>=1024&&i<units.length-1){n/=1024;i++;} return `${n.toFixed(i?1:0)} ${units[i]}${suffix.includes('/s')?'/s':''}`; }
      return `${value.toFixed(suffix === '%' ? 0 : 1)}${suffix}`;
    };
    return `<section class="card sensor-chart"><div class="card-head"><h2>${title}</h2><div class="chart-legend">${labels.map((label,i)=>`<span class="chart-legend-item"><i style="background:${colors[i%colors.length]}"></i>${escapeHtml(label)}</span>`).join('')}</div></div><div class="chart-scale"><span>${axis(max)}</span><span>${axis(min)}</span></div><svg viewBox="0 0 ${w} ${h}" role="img" aria-label="${escapeHtml(title)} history" preserveAspectRatio="none"><path d="M0 ${h/2} H${w}" stroke="var(--line)" stroke-dasharray="3 6"/>${keys.map((key,i)=>{ const pts=rows.map((row,j)=>{const value=row[key]??row.cores?.[key];return Number.isFinite(value)?`${pad+j*(w-2*pad)/Math.max(1,rows.length-1)},${h-pad-(value-min)*(h-2*pad)/(max-min)}`:null;}).filter(Boolean).join(' '); return pts?`<polyline points="${pts}" fill="none" stroke="${colors[i%colors.length]}" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/>`:''; }).join('')}</svg><div class="chart-time"><span>Oldest</span><span>Now</span></div></section>`;
  }
  function renderSensors() {
    const s = state.data || {}, t = state.telemetry || {};
    const sections = [
      ['overview','Overview'],['top-apps','Top apps'],['cpu','CPU'],['ram','RAM'],['gpu','GPU'],['storage','Storage'],['wifi','Wi-Fi'],['data-usage','Data usage'],['power','Power'],['bluetooth','Bluetooth'],['temperature','Temperature']
    ];
    const kb = (value) => value == null ? 'Unavailable' : `${(value / 1024 / 1024).toFixed(1)} GB`;
    const bytes = (value) => { if (value == null) return 'Unavailable'; const units=['B','KB','MB','GB','TB']; let n=value, i=0; while(n>=1024 && i<units.length-1){n/=1024;i++;} return `${n.toFixed(i ? 1 : 0)} ${units[i]}`; };
    const onOff = (value) => value == null ? 'Unavailable' : value ? 'On' : 'Off';
    const usage = s.mem_total_kb && s.mem_available_kb != null ? (100 * (s.mem_total_kb - s.mem_available_kb) / s.mem_total_kb) : null;
    const totalStorage = s.storage_internal_total_kb == null ? t.storage_internal_total_gb : s.storage_internal_total_kb / 1024 / 1024;
    const freeStorage = s.storage_internal_free_kb == null ? t.storage_internal_free_gb : s.storage_internal_free_kb / 1024 / 1024;
    const storageUsage = totalStorage && freeStorage != null ? 100 * (1 - freeStorage / totalStorage) : null;
    const network = s.network_interfaces || [], temps = s.temperatures || [], processes = s.top_processes || [], sensors = s.sensor_names || [];
    const grouped = new Map();
    for (const p of processes) { const name = p.name || 'Unknown process', app = name.split(':')[0], row = grouped.get(app) || { name: app, pids: [], cpu: 0, memory: 0 }; row.pids.push(p.pid); row.cpu += Number(p.cpu_pct) || 0; row.memory += Number(p.memory_kb) || 0; grouped.set(app, row); }
    const apps = [...grouped.values()].filter(p => !state.sensorFilter || p.name.toLowerCase().includes(state.sensorFilter.toLowerCase()));
    apps.sort((a,b) => state.sensorSort === 'memory' ? b.memory-a.memory : state.sensorSort === 'name' ? a.name.localeCompare(b.name) : b.cpu-a.cpu);
    const topAppsCard = `<section class="card"><div class="card-head"><div><h2>Top apps</h2><div class="subtle">Grouped Android processes · current sample · ${grouped.size} apps</div></div><div class="toolbar compact"><input class="input" id="sensor-app-filter" value="${escapeHtml(state.sensorFilter)}" placeholder="Filter apps"><select class="input sensor-sort" id="sensor-app-sort"><option value="cpu" ${state.sensorSort==='cpu'?'selected':''}>Sort: CPU</option><option value="memory" ${state.sensorSort==='memory'?'selected':''}>Sort: Memory</option><option value="name" ${state.sensorSort==='name'?'selected':''}>Sort: Name</option></select></div></div>${apps.length ? `<div class="table-wrap"><table class="table"><thead><tr><th>App / process</th><th>Processes</th><th>CPU</th><th>Memory</th></tr></thead><tbody>${apps.slice(0,30).map(p=>`<tr><td>${escapeHtml(p.name)}</td><td>${p.pids.length}</td><td>${p.cpu.toFixed(1)}%</td><td>${bytes(p.memory*1024)}</td></tr>`).join('')}</tbody></table></div>` : empty('No matching processes','Try another filter or refresh the device sample.')}</section>`;
    const cards = {
      'top-apps': topAppsCard,
      cpu: `<div class="grid metrics">${metric('Processor',escapeHtml(val(s.cpu_model)),`${s.cpu_cores || '—'} logical cores`,null)}${metric('Current load',s.cpu_usage_pct == null ? 'Unavailable' : `${escapeHtml(s.cpu_usage_pct.toFixed(1))}%`,'Sample from Android top',s.cpu_usage_pct)}${metric('Architecture',escapeHtml(val(t.cpu_abi)),'Device ABI',null)}${metric('Core frequency',s.cpu_frequencies_mhz?.length ? `${Math.min(...s.cpu_frequencies_mhz)}–${Math.max(...s.cpu_frequencies_mhz)} MHz` : 'Unavailable','Current scaling frequency',null)}</div>${sensorChart('CPU load history',['cpu'],['Total'])}${sensorChart('Per-core load',Array.from({length:s.cpu_cores||0},(_,i)=>`cpu${i}`),Array.from({length:s.cpu_cores||0},(_,i)=>`Core ${i}`))}${topAppsCard}`,
      ram: `<div class="grid metrics">${metric('Total RAM',kb(s.mem_total_kb),'Android kernel memory',null)}${metric('Available',kb(s.mem_available_kb),'Memory available to apps',usage == null ? null : 100-usage)}${metric('In use',usage == null ? 'Unavailable' : `${usage.toFixed(1)}%`,'Calculated from MemAvailable',usage)}${metric('Swap',s.swap_total_kb == null ? 'Unavailable' : `${kb(s.swap_total_kb - (s.swap_free_kb || 0))} / ${kb(s.swap_total_kb)}`,'Used / total',null)}</div><div class="grid two">${sensorChart('RAM utilization',['ram'],['RAM'])}${sensorChart('Swap utilization',['swap'],['Swap'])}</div>`,
      gpu: `<div class="grid two">${card('Graphics renderer',`<div class="kv"><div><small>OpenGL renderer</small><b>${escapeHtml(val(s.gpu_renderer))}</b></div><div><small>Platform</small><b>${escapeHtml(val(s.gpu_platform))}</b></div></div>`)}${card('Graphics telemetry',empty('GPU load unavailable','Android does not provide a standard unprivileged ADB interface for live GPU load and temperature. Renderer and platform identifiers are shown when exposed by the device.'))}</div>`,
      storage: `<div class="grid metrics">${metric('Internal used',totalStorage != null && freeStorage != null ? `${(totalStorage-freeStorage).toFixed(1)} GB` : 'Unavailable',totalStorage != null ? `${freeStorage.toFixed(1)} GB available of ${totalStorage.toFixed(1)} GB` : 'Device storage telemetry unavailable',storageUsage)}${metric('Shared storage',s.storage_shared_total_kb != null ? `${((s.storage_shared_total_kb-s.storage_shared_free_kb)/1024/1024).toFixed(1)} GB` : 'Unavailable',s.storage_shared_total_kb != null ? `${(s.storage_shared_free_kb/1024/1024).toFixed(1)} GB free` : 'No shared storage telemetry',null)}</div>${sensorChart('Storage utilization',['storage'],['Used'])}${sensorChart('Disk throughput',['diskReadRate','diskWriteRate'],['Read','Write'],' B/s')}`,
      wifi: `<div class="grid two">${sensorChart('Network throughput',['rxRate','txRate'],['Received','Sent'],' B/s')}${card('Wireless status',`<div class="kv"><div><small>Wi-Fi radio</small><b>${onOff(s.wifi_enabled)}</b></div><div><small>Interfaces</small><b>${network.length}</b></div></div>`)}${card('Network interfaces',network.length ? `<div class="table-wrap"><table class="table"><thead><tr><th>Interface</th><th>Received</th><th>Sent</th></tr></thead><tbody>${network.map(n=>`<tr><td>${escapeHtml(n.name)}</td><td>${bytes(n.rx_bytes)}</td><td>${bytes(n.tx_bytes)}</td></tr>`).join('')}</tbody></table></div>` : empty('Network counters unavailable','The device did not expose /proc/net/dev to the ADB shell.'))}</div>`,
      'data-usage': `${sensorChart('Cumulative interface traffic',['rx','tx'],['Received','Transmitted'],' bytes')}<section class="card"><div class="card-head"><div><h2>Interface data counters</h2><div class="subtle">Cumulative bytes reported by Android since boot; includes system traffic.</div></div></div>${network.length ? `<div class="table-wrap"><table class="table"><thead><tr><th>Interface</th><th>Received</th><th>Transmitted</th><th>Total</th></tr></thead><tbody>${network.map(n=>`<tr><td>${escapeHtml(n.name)}</td><td>${bytes(n.rx_bytes)}</td><td>${bytes(n.tx_bytes)}</td><td>${bytes(n.rx_bytes+n.tx_bytes)}</td></tr>`).join('')}</tbody></table></div>` : empty('Data counters unavailable','Android did not expose interface byte counters.')}</section>`,
      power: `<div class="grid metrics">${metric('Battery',val(s.battery_pct,'%'),escapeHtml(val(s.battery_status)),s.battery_pct)}${metric('Battery temperature',val(s.battery_temp_c,' °C'),escapeHtml(val(s.battery_health ? `Health code ${s.battery_health}` : null)),null)}${metric('Voltage',s.battery_voltage_mv == null ? 'Unavailable' : `${(s.battery_voltage_mv/1000).toFixed(2)} V`,escapeHtml(val(s.battery_plugged)),null)}${metric('Uptime',s.uptime_secs ? `${Math.floor(s.uptime_secs/3600)}h ${Math.floor(s.uptime_secs%3600/60)}m` : 'Unavailable','Since last boot',null)}</div><div class="grid two">${sensorChart('Battery level',['battery'],['Charge'])}${sensorChart('Battery temperature',['temp'],['Temperature'],' °C')}</div>`,
      bluetooth: `<section class="card"><div class="card-head"><h2>Bluetooth radio</h2></div><div class="grid metrics">${metric('State',onOff(s.bluetooth_enabled),'Android global Bluetooth setting',null)}${metric('Adapter',s.bluetooth_enabled == null ? 'Unavailable' : s.bluetooth_enabled ? 'Enabled' : 'Disabled','Radio state',null)}</div></section>`,
      temperature: `${sensorChart('Temperature history',['temp'],['Warmest sensor'],' °C')}<section class="card"><div class="card-head"><div><h2>Thermal sensors</h2><div class="subtle">Read from Android ThermalService and readable kernel thermal zones.</div></div></div>${temps.length ? `<div class="photo-grid">${temps.map(x=>`<article class="list-item"><div class="list-title">${escapeHtml(x.name)}</div><div class="metric-value">${escapeHtml(x.temp_c.toFixed(1))} °C</div></article>`).join('')}</div>` : empty('Thermal readings unavailable','Android did not expose current thermal zone values.')}</section><section class="card"><div class="card-head"><h2>Registered sensors</h2><span class="chip">${sensors.length} reported</span></div>${sensors.length ? `<div class="sensor-tags">${sensors.map(name=>`<span class="chip">${escapeHtml(name)}</span>`).join('')}</div>` : empty('Sensor inventory unavailable','The device does not expose its sensor list through dumpsys sensorservice.')}</section>`
    };
    const overviewCards = `<div class="grid metrics">${metric('CPU',escapeHtml(val(s.cpu_model)),`${s.cpu_cores || '—'} cores · ${s.cpu_usage_pct == null ? 'load unavailable' : `${escapeHtml(s.cpu_usage_pct.toFixed(1))}% sampled`}`,s.cpu_usage_pct)}${metric('RAM',usage == null ? 'Unavailable' : `${usage.toFixed(1)}% used`,`${kb(s.mem_available_kb)} available of ${kb(s.mem_total_kb)}`,usage)}${metric('Storage',totalStorage != null && freeStorage != null ? `${(totalStorage-freeStorage).toFixed(1)} GB used` : 'Unavailable',totalStorage != null ? `${freeStorage.toFixed(1)} GB available` : 'Device storage telemetry unavailable',storageUsage)}${metric('Battery',val(s.battery_pct,'%'),escapeHtml(val(s.battery_status)),s.battery_pct)}</div><div class="grid two">${sensorChart('CPU load',['cpu'],['CPU'])}${sensorChart('Memory usage',['ram','swap'],['RAM','Swap'])}${sensorChart('Network throughput',['rxRate','txRate'],['Received','Sent'],' B/s')}${sensorChart('Thermal trend',['temp'],['Warmest sensor'],' °C')}</div><div class="grid two">${card('Connectivity',`<div class="kv"><div><small>Wi-Fi</small><b>${onOff(s.wifi_enabled)}</b></div><div><small>Bluetooth</small><b>${onOff(s.bluetooth_enabled)}</b></div><div><small>Interfaces</small><b>${network.length}</b></div><div><small>Data transferred</small><b>${bytes(network.reduce((n,x)=>n+x.rx_bytes+x.tx_bytes,0))}</b></div></div>`)}${card('Thermals & sensors',`<div class="kv"><div><small>Temperature sensors</small><b>${temps.length || 'Unavailable'}</b></div><div><small>Registered sensors</small><b>${sensors.length || 'Unavailable'}</b></div><div><small>Battery temperature</small><b>${val(s.battery_temp_c,' °C')}</b></div><div><small>GPU renderer</small><b>${escapeHtml(val(s.gpu_renderer))}</b></div></div>`)}</div>${topAppsCard}`;
    const active = state.sensorsView in cards ? cards[state.sensorsView] : overviewCards;
    content.innerHTML = `<section class="sensor-toolbar"><div><h2>Device sensors</h2><span class="subtle">Live readings from Android system services and kernel interfaces</span></div><button class="button" data-refresh>${icon('e145')} Refresh</button></section><nav class="sensor-nav" aria-label="Sensor categories">${sections.map(([key,label])=>`<button class="sensor-tab ${state.sensorsView===key?'active':''}" data-sensor-view="${key}">${escapeHtml(label)}</button>`).join('')}</nav><div class="sensor-panel">${active}</div>`;
    const filter = $('#sensor-app-filter'), sort = $('#sensor-app-sort');
    if (filter) filter.oninput = () => { state.sensorFilter = filter.value; renderSensors(); const next = $('#sensor-app-filter'); next.focus(); next.setSelectionRange(next.value.length, next.value.length); };
    if (sort) sort.onchange = () => { state.sensorSort = sort.value; renderSensors(); };
    clearTimeout(state.sensorTimer);
    if (state.tab === 'sensors' && !document.hidden) state.sensorTimer = setTimeout(async () => {
      if (state.sensorBusy || state.tab !== 'sensors' || document.hidden) return;
      state.sensorBusy = true;
      try { await loadData('sensors'); if (state.tab === 'sensors' && !document.activeElement?.matches('#sensor-app-filter')) renderSensors(); }
      finally { state.sensorBusy = false; }
    }, 5000);
  }
  async function refreshCurrent() { if (state.tab === 'overview' || state.tab === 'storage') await refreshTelemetry(); else await renderTab(); }

  document.addEventListener('click', async (event) => {
    const sensorTab = event.target.closest('[data-sensor-view]'); if (sensorTab) { state.sensorsView=sensorTab.dataset.sensorView; renderSensors(); return; }
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
  document.addEventListener('visibilitychange', () => { if(document.hidden){clearInterval(state.screenTimer);state.screenTimer=null;clearTimeout(state.sensorTimer);state.sensorTimer=null;} else if(state.tab==='screen'&&state.serial) selectTab('screen'); else if(state.tab==='sensors'&&state.serial) void renderTab(); });
  (async () => {
    if (!invoke) { content.innerHTML=`<div class="card empty"><b>Tauri runtime is unavailable</b><span>Launch the Android cockpit from Server Launcher.</span></div>`; return; }
    try {
      state.serial=await invoke('adb_initial_serial') || '';
      const accent=await invoke('adb_accent_color');
      if (/^#[\da-f]{6}$/i.test(accent || '')) {
        document.documentElement.style.setProperty('--accent', accent);
        document.documentElement.style.setProperty('--accent-strong', accent);
      }
      await refreshDevices();
    } catch(e) { toast(String(e),true); }
    setInterval(async () => { if(document.hidden)return; const old=state.serial; await refreshDevices(); if(state.serial===old && !['screen','sensors'].includes(state.tab) && Date.now()-state.lastTelemetry>30000) await refreshTelemetry(); }, 7000);
  })();
})();
