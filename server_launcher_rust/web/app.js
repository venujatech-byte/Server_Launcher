(() => {
  const invoke = window.__TAURI__?.core?.invoke;
  const $ = (selector) => document.querySelector(selector);
  const content = $('#content');
  const state = { serial: '', device: null, devices: [], tab: 'overview', telemetry: null, data: [], messageAddress: '', sensorsView: 'overview', sensorsCadence: 2000, sensorHistory: [], sensorTimer: null, sensorBusy: false, sensorFilter: '', sensorSort: 'cpu', sensorInventory: [], sensorInventorySerial: '', sensorInventoryLoading: false, sensorInventoryLoaded: false, path: '/sdcard', screenTimer: null, toastTimer: null, lastTelemetry: 0 };
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
      if (state.sensorInventorySerial !== state.serial) { state.sensorInventory = []; state.sensorInventorySerial = state.serial; state.sensorInventoryLoaded = false; }
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
    const conversations = [...rows.reduce((groups, message) => {
      const address = message.address || 'Unknown contact';
      if (!groups.has(address)) groups.set(address, []);
      groups.get(address).push(message);
      return groups;
    }, new Map())].map(([address, messages]) => ({ address, messages, latest: messages[0] }));
    if (!conversations.some((conversation) => conversation.address === state.messageAddress)) state.messageAddress = conversations[0]?.address || '';
    const selected = conversations.find((conversation) => conversation.address === state.messageAddress);
    content.innerHTML = `<div class="comm-page"><header class="comm-heading"><div><div class="eyebrow">DEVICE COMMUNICATIONS</div><h2>Messages</h2><p class="subtle">Your SMS conversations from this device</p></div><button class="button" data-refresh>${icon("e145")} Refresh</button></header><div class="comm-layout messages-layout"><section class="card comm-list-pane"><div class="card-head"><div><h3>Conversations</h3><span class="subtle">${conversations.length} threads · ${rows.length} messages</span></div></div><input id="message-filter" class="input" type="search" placeholder="Search conversations" aria-label="Search conversations"><div class="comm-list">${conversations.length ? conversations.map((conversation) => `<button class="comm-row ${conversation.address === state.messageAddress ? 'selected' : ''}" data-message-address="${escapeHtml(conversation.address)}"><span class="comm-avatar">${escapeHtml((conversation.latest.contact || conversation.address).slice(0, 1).toUpperCase())}</span><span class="comm-row-copy"><span class="comm-row-top"><b>${escapeHtml(conversation.latest.contact || conversation.address)}</b><time>${escapeHtml(conversation.latest.time)}</time></span><span class="comm-preview">${conversation.latest.outgoing ? 'You: ' : ''}${escapeHtml(conversation.latest.text)}</span></span></button>`).join('') : empty('No messages available', 'SMS conversations will appear here when the device provides them.')}</div></section><section class="card comm-thread-pane">${selected ? `<div class="thread-head"><span class="comm-avatar">${escapeHtml((selected.latest.contact || selected.address).slice(0, 1).toUpperCase())}</span><div><h3>${escapeHtml(selected.latest.contact || selected.address)}</h3><span class="subtle">${escapeHtml(selected.address)}</span></div></div><div class="message-thread">${selected.messages.slice().reverse().map((message) => `<article class="message-bubble ${message.outgoing ? 'outgoing' : 'incoming'}"><p>${escapeHtml(message.text)}</p><time>${escapeHtml(message.time)}</time></article>`).join('')}</div><form id="sms-form" class="message-composer"><label class="subtle" for="sms-number">To</label><input id="sms-number" class="input" value="${escapeHtml(selected.address)}" aria-label="Recipient phone number"><div class="composer-row"><textarea id="sms-body" class="input" rows="2" placeholder="Write a message…" aria-label="Message"></textarea><button class="button primary" id="sms-send" type="submit">${icon("e1e0")} Open SMS app</button></div><span class="subtle">Review and send in your device’s messaging app. Carrier charges may apply.</span></form>` : `<div class="thread-empty">${empty('Select a conversation', 'Choose a conversation to read or reply.')}</div>`}</section></div></div>`;
    $('#message-filter').oninput = (event) => document.querySelectorAll('.comm-row').forEach((row) => { row.hidden = !row.textContent.toLowerCase().includes(event.target.value.toLowerCase()); });
    document.querySelectorAll('[data-message-address]').forEach((button) => button.onclick = () => { state.messageAddress = button.dataset.messageAddress; renderMessages(); });
    $('#sms-form')?.addEventListener('submit', async (event) => { event.preventDefault(); const number = $('#sms-number').value.trim(), body = $('#sms-body').value.trim(); if (!number || !body) return toast('Enter a number and message.', true); const send = $('#sms-send'); send.disabled = true; try { await action('send_sms', number, body); } catch {} finally { if ($('#sms-send')) $('#sms-send').disabled = false; } });
  }
  function renderCalls() {
    const rows = state.data || [];
    content.innerHTML = `<div class="comm-page"><header class="comm-heading"><div><div class="eyebrow">DEVICE COMMUNICATIONS</div><h2>Calls</h2><p class="subtle">Recent calls and phone keypad</p></div><button class="button" data-refresh>${icon("e145")} Refresh</button></header><div class="comm-layout calls-layout"><section class="card comm-list-pane"><div class="card-head"><div><h3>Recent calls</h3><span class="subtle">${rows.length} entries</span></div></div><input id="call-filter" class="input" type="search" placeholder="Search calls" aria-label="Search recent calls"><div id="call-list" class="comm-list">${rows.length ? rows.map((call) => `<article class="comm-row call-row"><span class="comm-avatar call-avatar">${icon(call.kind === 'Missed' ? 'e11b' : 'e135')}</span><span class="comm-row-copy"><span class="comm-row-top"><b>${escapeHtml(call.name || call.number || 'Unknown caller')}</b><time>${escapeHtml(call.time)}</time></span><span class="comm-preview">${escapeHtml(call.kind)} · ${escapeHtml(call.duration)} · ${escapeHtml(call.number)}</span></span><button class="button tonal call-back" data-action="call" data-value="${escapeHtml(call.number)}" aria-label="Call ${escapeHtml(call.name || call.number)}">${icon("e135")}</button></article>`).join('') : empty('No call history', 'Call log records will appear here when available.')}</div></section><section class="card comm-dial-pane"><div class="dialer-title"><h3>Dial a number</h3><span class="badge">ANDROID PHONE</span></div><label class="subtle" for="dial-number">Phone number</label><input id="dial-number" class="input dial-input" placeholder="Enter a phone number" inputmode="tel" autocomplete="tel"><div class="keypad">${['1','2','3','4','5','6','7','8','9','*','0','#'].map((key) => `<button class="button key" data-key="${key}" aria-label="${key}">${key}</button>`).join('')}</div><button class="button success dial-submit" id="dial-call">${icon("e135")} Call</button><button class="button danger dial-end" data-action="hangup">End current call</button><p class="subtle dial-note">Calling uses the connected Android device.</p></section></div></div>`;
    $('#call-filter').oninput = (e) => document.querySelectorAll('.call-row').forEach(row => row.hidden = !row.textContent.toLowerCase().includes(e.target.value.toLowerCase()));
    document.querySelectorAll('[data-key]').forEach(k => k.onclick = () => { const input = $('#dial-number'); input.value += k.dataset.key; input.focus(); });
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
  function formatRate(value) {
    if (value == null || !Number.isFinite(value)) return '0 B/s';
    const units = ['B/s', 'KB/s', 'MB/s', 'GB/s'];
    let n = Math.abs(value), i = 0;
    while (n >= 1024 && i < units.length - 1) { n /= 1024; i++; }
    return `${(value < 0 ? -1 : 1 * n).toFixed(i ? 1 : 0)} ${units[i]}`;
  }

  function formatBytes(value) {
    if (value == null || !Number.isFinite(value)) return '0 B';
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    let n = Math.abs(value), i = 0;
    while (n >= 1024 && i < units.length - 1) { n /= 1024; i++; }
    return `${(value < 0 ? -1 : 1 * n).toFixed(i ? 1 : 0)} ${units[i]}`;
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
    const seconds = previous ? Math.max(0.1, (now - previous.at) / 1000) : 0;
    const rate = (current, old) => previous && current >= old && seconds > 0 ? (current - old) / seconds : 0;
    const temps = s.temperatures || [];
    const cores = {};
    let procCpu = null;
    for (const c of s.cpu_counters || []) {
      if (c.name === 'cpu') {
        const oldTotal = previous?.cpuCounters?.cpu?.total, oldIdle = previous?.cpuCounters?.cpu?.idle;
        if (oldTotal != null && c.total > oldTotal && c.idle >= oldIdle) {
          procCpu = 100 * (1 - (c.idle - oldIdle) / (c.total - oldTotal));
        }
        continue;
      }
      if (!previous) continue;
      const old = (previous.cpuCounters || {})[c.name];
      if (old && c.total > old.total && c.idle >= old.idle) {
        cores[c.name] = 100 * (1 - (c.idle - old.idle) / (c.total - old.total));
      }
    }
    const socketTotal = Number(s.socket_total) || 0;
    const socketTcp = Number(s.socket_tcp) || 0;
    const socketUdp = Number(s.socket_udp) || 0;
    const rxRate = rate(rx, previous?.rx ?? rx);
    const txRate = rate(tx, previous?.tx ?? tx);
    const diskReadRate = rate(diskRead, previous?.diskRead ?? diskRead) * 512;
    const diskWriteRate = rate(diskWrite, previous?.diskWrite ?? diskWrite) * 512;

    state.sensorHistory.push({
      at: now,
      cpu: s.cpu_usage_pct ?? procCpu ?? 0,
      ram: mem ?? 0,
      swap: s.swap_total_kb ? 100 * (s.swap_total_kb - (s.swap_free_kb || 0)) / s.swap_total_kb : 0,
      storage: diskTotal && diskFree != null ? 100 * (diskTotal - diskFree) / diskTotal : 0,
      rx, tx, rxRate, txRate,
      diskReadRate, diskWriteRate,
      battery: s.battery_pct ?? 0,
      temp: temps.length ? Math.max(...temps.map(x => Number(x.temp_c)).filter(Number.isFinite)) : (s.battery_temp_c ?? 0),
      cores,
      cpuCounters: Object.fromEntries((s.cpu_counters || []).map(c => [c.name, { total: c.total, idle: c.idle }])),
      diskRead, diskWrite,
      socketTotal, socketTcp, socketUdp
    });
    if (state.sensorHistory.length > 60) state.sensorHistory.shift();
  }

  function renderSparklineSvg(data = [], maxVal = 100, dottedRatio = 0.5, color = 'var(--accent)') {
    const pointsData = (data.length === 1 ? [data[0], data[0]] : data).map(v => Number(v)).filter(Number.isFinite);
    if (pointsData.length < 2) {
      return `<svg class="stat-card-spark" viewBox="0 0 100 32" preserveAspectRatio="none"><line x1="0" y1="16" x2="100" y2="16" stroke="rgba(255,255,255,0.06)" stroke-width="1"/></svg>`;
    }
    const max = Math.max(maxVal || 1, ...pointsData);
    const min = Math.min(0, ...pointsData);
    const range = max - min || 1;
    const n = pointsData.length;
    const stepX = 100 / (n - 1);
    const pts = pointsData.map((v, i) => {
      const x = (i * stepX).toFixed(1);
      const norm = Math.max(0.02, Math.min(0.96, (v - min) / range));
      const y = (31 - norm * 29).toFixed(1);
      return `${x},${y}`;
    }).join(' ');

    const gradId = 'spk-' + Math.random().toString(36).slice(2, 7);
    const dottedLine = dottedRatio != null
      ? `<line x1="0" y1="${(31 - Math.max(0.05, Math.min(0.95, dottedRatio)) * 29).toFixed(1)}" x2="100" y2="${(31 - Math.max(0.05, Math.min(0.95, dottedRatio)) * 29).toFixed(1)}" stroke="${color}" stroke-opacity="0.4" stroke-dasharray="3,3" stroke-width="1"/>`
      : '';

    return `
      <svg class="stat-card-spark" viewBox="0 0 100 32" preserveAspectRatio="none" aria-hidden="true">
        <defs>
          <linearGradient id="${gradId}" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stop-color="${color}" stop-opacity="0.35"/>
            <stop offset="100%" stop-color="${color}" stop-opacity="0.02"/>
          </linearGradient>
        </defs>
        ${dottedLine}
        <polygon points="0,32 ${pts} 100,32" fill="url(#${gradId})"/>
        <polyline points="${pts}" fill="none" stroke="${color}" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" vector-effect="non-scaling-stroke"/>
      </svg>
    `;
  }

  function renderBigStatCard({ iconCode, title, heroVal, heroUnit = '', subtitle = '', leftStat = '', rightStat = '', data = [], maxVal = 100, dottedRatio = 0.5, color = 'var(--accent)' }) {
    const spark = renderSparklineSvg(data, maxVal, dottedRatio, color);
    return `
      <article class="sensors-stat-card">
        <div class="stat-card-title"><span class="icon">${icon(iconCode)}</span><span>${escapeHtml(title)}</span></div>
        <div class="stat-card-hero">
          <span class="stat-hero-val">${escapeHtml(heroVal)}</span>
          ${heroUnit ? `<span class="stat-hero-unit">${escapeHtml(heroUnit)}</span>` : ''}
        </div>
        <div class="stat-card-sub" title="${escapeHtml(subtitle)}">${escapeHtml(subtitle)}</div>
        <div class="stat-card-range">
          <span>${escapeHtml(leftStat)}</span>
          <span>${escapeHtml(rightStat)}</span>
        </div>
        <div class="stat-card-spark">${spark}</div>
      </article>
    `;
  }

  function renderDualNetworkSvg(txHistory = [], rxHistory = [], peakVal = 0, accentColor = 'var(--accent)') {
    const n = Math.max(txHistory.length, rxHistory.length);
    const h = 130, w = 500;
    if (n < 2) {
      return `<div class="chart-empty">Collecting network telemetry samples…</div>`;
    }
    const maxV = Math.max(peakVal || 0, ...txHistory, ...rxHistory, 1024);
    const stepX = (w - 20) / (n - 1);
    const yBase = h - 14;
    const yTop = 14;
    const graphH = yBase - yTop;

    const toPoints = (series) => series.map((v, i) => {
      const x = (10 + i * stepX).toFixed(1);
      const norm = Math.max(0.02, Math.min(0.94, (Number(v) || 0) / maxV));
      const y = (yBase - norm * graphH).toFixed(1);
      return `${x},${y}`;
    });

    const txPts = toPoints(txHistory);
    const rxPts = toPoints(rxHistory);

    const txPoly = txPts.join(' ');
    const rxPoly = rxPts.join(' ');

    const txArea = `10,${yBase} ${txPoly} ${10 + (txHistory.length - 1) * stepX},${yBase}`;
    const rxArea = `10,${yBase} ${rxPoly} ${10 + (rxHistory.length - 1) * stepX},${yBase}`;

    const gradTx = 'dnet-tx-' + Math.random().toString(36).slice(2, 7);
    const gradRx = 'dnet-rx-' + Math.random().toString(36).slice(2, 7);
    const yPeak = (yTop + graphH * 0.28).toFixed(1);

    return `
      <svg viewBox="0 0 ${w} ${h}" role="img" aria-label="Network throughput history" preserveAspectRatio="none" style="width:100%;height:130px;display:block;">
        <defs>
          <linearGradient id="${gradTx}" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stop-color="${accentColor}" stop-opacity="0.32"/>
            <stop offset="100%" stop-color="${accentColor}" stop-opacity="0.02"/>
          </linearGradient>
          <linearGradient id="${gradRx}" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stop-color="#38bdf8" stop-opacity="0.18"/>
            <stop offset="100%" stop-color="#38bdf8" stop-opacity="0.01"/>
          </linearGradient>
        </defs>
        <line x1="10" y1="${yBase}" x2="${w - 10}" y2="${yBase}" stroke="rgba(255,255,255,0.08)" stroke-width="1"/>
        <line x1="10" y1="${yPeak}" x2="${w - 10}" y2="${yPeak}" stroke="${accentColor}" stroke-opacity="0.45" stroke-dasharray="5,4" stroke-width="1.2"/>
        <polygon points="${rxArea}" fill="url(#${gradRx})"/>
        <polyline points="${rxPoly}" fill="none" stroke="#38bdf8" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>
        <polygon points="${txArea}" fill="url(#${gradTx})"/>
        <polyline points="${txPoly}" fill="none" stroke="${accentColor}" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/>
      </svg>
    `;
  }

  function renderSocketsChartSvg(tcpHistory = [], udpHistory = [], accentColor = 'var(--accent)') {
    const n = Math.max(tcpHistory.length, udpHistory.length);
    const h = 130, w = 350;
    if (n < 2) {
      return `<div class="chart-empty">Collecting socket telemetry…</div>`;
    }
    const maxV = Math.max(10, ...tcpHistory, ...udpHistory);
    const stepX = (w - 20) / (n - 1);
    const yBase = h - 14;
    const yTop = 14;
    const graphH = yBase - yTop;

    const toPoints = (series) => series.map((v, i) => {
      const x = (10 + i * stepX).toFixed(1);
      const norm = Math.max(0.04, Math.min(0.92, (Number(v) || 0) / maxV));
      const y = (yBase - norm * graphH).toFixed(1);
      return `${x},${y}`;
    });

    const tcpPts = toPoints(tcpHistory);
    const udpPts = toPoints(udpHistory);

    const tcpPoly = tcpPts.join(' ');
    const udpPoly = udpPts.join(' ');

    const tcpArea = `10,${yBase} ${tcpPoly} ${10 + (tcpHistory.length - 1) * stepX},${yBase}`;
    const gradTcp = 'sock-tcp-' + Math.random().toString(36).slice(2, 7);
    const yMid = (yTop + graphH * 0.45).toFixed(1);

    return `
      <svg viewBox="0 0 ${w} ${h}" role="img" aria-label="Sockets history" preserveAspectRatio="none" style="width:100%;height:130px;display:block;">
        <defs>
          <linearGradient id="${gradTcp}" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stop-color="${accentColor}" stop-opacity="0.32"/>
            <stop offset="100%" stop-color="${accentColor}" stop-opacity="0.02"/>
          </linearGradient>
        </defs>
        <line x1="10" y1="${yBase}" x2="${w - 10}" y2="${yBase}" stroke="rgba(255,255,255,0.08)" stroke-width="1"/>
        <line x1="10" y1="${yMid}" x2="${w - 10}" y2="${yMid}" stroke="${accentColor}" stroke-opacity="0.35" stroke-dasharray="4,4" stroke-width="1.2"/>
        <polygon points="${tcpArea}" fill="url(#${gradTcp})"/>
        <polyline points="${tcpPoly}" fill="none" stroke="${accentColor}" stroke-width="2.0" stroke-linecap="round" stroke-linejoin="round"/>
        <polyline points="${udpPoly}" fill="none" stroke="#94a3b8" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" stroke-dasharray="4,3"/>
      </svg>
    `;
  }

  function renderWaveformChartSvg({ title, series = [], height = 140, suffix = '%', benchmarkVal = null }) {
    const n = Math.max(...series.map(s => s.data?.length || 0));
    const w = 700, h = height;
    if (n < 2) {
      return `
        <section class="card sensors-waveform-card">
          <div class="card-head"><h2>${escapeHtml(title)}</h2><span class="subtle">Collecting samples · ${n}/2</span></div>
          <div class="waveform-chart-box"><div class="chart-empty">Waveform appears after two live samples.</div></div>
        </section>
      `;
    }
    const allValues = series.flatMap(s => s.data || []).map(Number).filter(Number.isFinite);
    let maxV = Math.max(...allValues, 1);
    let minV = suffix === ' °C' ? Math.floor(Math.min(...allValues) - 2) : Math.min(0, ...allValues);
    if (suffix === '%' && maxV < 100) maxV = 100;
    if (maxV === minV) maxV = minV + 1;
    const range = maxV - minV;

    const yBase = h - 14;
    const yTop = 14;
    const graphH = yBase - yTop;
    const stepX = (w - 20) / (n - 1);

    const legend = series.map((s, idx) => `
      <span class="chart-legend-item">
        <i style="background:${s.color || 'var(--accent)'}"></i>
        ${escapeHtml(s.label)}
      </span>
    `).join('');

    const gridLines = [0.25, 0.5, 0.75].map(ratio => {
      const y = (yBase - ratio * graphH).toFixed(1);
      return `<line x1="10" y1="${y}" x2="${w - 10}" y2="${y}" stroke="rgba(255,255,255,0.06)" stroke-dasharray="4,4" stroke-width="1"/>`;
    }).join('');

    const benchmarkLine = benchmarkVal != null
      ? `<line x1="10" y1="${(yBase - Math.max(0.05, Math.min(0.95, (benchmarkVal - minV) / range)) * graphH).toFixed(1)}" x2="${w - 10}" y2="${(yBase - Math.max(0.05, Math.min(0.95, (benchmarkVal - minV) / range)) * graphH).toFixed(1)}" stroke="var(--accent)" stroke-opacity="0.45" stroke-dasharray="6,4" stroke-width="1.2"/>`
      : '';

    const paths = series.map((s, idx) => {
      const d = s.data || [];
      const pts = d.map((v, i) => {
        const x = (10 + i * stepX).toFixed(1);
        const norm = Math.max(0.02, Math.min(0.98, ((Number(v) || 0) - minV) / range));
        const y = (yBase - norm * graphH).toFixed(1);
        return `${x},${y}`;
      });
      const poly = pts.join(' ');
      const area = `10,${yBase} ${poly} ${10 + (d.length - 1) * stepX},${yBase}`;
      const gradId = 'wf-grad-' + idx + '-' + Math.random().toString(36).slice(2, 6);
      return `
        <defs>
          <linearGradient id="${gradId}" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stop-color="${s.color || 'var(--accent)'}" stop-opacity="${s.fillOpacity ?? 0.28}"/>
            <stop offset="100%" stop-color="${s.color || 'var(--accent)'}" stop-opacity="0.01"/>
          </linearGradient>
        </defs>
        <polygon points="${area}" fill="url(#${gradId})"/>
        <polyline points="${poly}" fill="none" stroke="${s.color || 'var(--accent)'}" stroke-width="${s.strokeWidth || 2.0}" stroke-linecap="round" stroke-linejoin="round"/>
      `;
    }).join('');

    return `
      <section class="card sensors-waveform-card">
        <div class="card-head">
          <h2>${escapeHtml(title)}</h2>
          <div class="chart-legend">${legend}</div>
        </div>
        <div class="waveform-chart-box">
          <svg viewBox="0 0 ${w} ${h}" role="img" aria-label="${escapeHtml(title)}" preserveAspectRatio="none" style="width:100%;height:${h}px;display:block;">
            <line x1="10" y1="${yBase}" x2="${w - 10}" y2="${yBase}" stroke="rgba(255,255,255,0.08)" stroke-width="1"/>
            ${gridLines}
            ${benchmarkLine}
            ${paths}
          </svg>
        </div>
        <div class="waveform-time-axis">
          <span>60s ago</span>
          <span>45s</span>
          <span>30s</span>
          <span>15s</span>
          <span>Now</span>
        </div>
      </section>
    `;
  }

  async function pollSensors() {
    if (state.sensorBusy || state.tab !== 'sensors' || document.hidden || state.sensorsCadence === 0) return;
    state.sensorBusy = true;
    try {
      await loadData('sensors');
      if (state.tab === 'sensors' && !document.activeElement?.matches('#sensor-app-filter')) {
        renderSensors();
      }
    } finally {
      state.sensorBusy = false;
      if (state.tab === 'sensors' && state.sensorsCadence > 0 && !document.hidden) {
        clearTimeout(state.sensorTimer);
        state.sensorTimer = setTimeout(pollSensors, state.sensorsCadence);
      }
    }
  }

  function renderSensors() {
    const s = state.data || {}, t = state.telemetry || {};
    const subtabs = [
      ['overview', 'Overview', 'e186'],
      ['top-apps', 'Top Apps', 'e129'],
      ['cpu', 'CPU', 'e181'],
      ['ram', 'RAM', 'e085'],
      ['gpu', 'GPU', 'e0f7'],
      ['storage', 'Storage', 'e0c5'],
      ['wifi', 'Wi-Fi', 'e1ae'],
      ['data-usage', 'Data Usage', 'e119'],
      ['power', 'Power', 'e145'],
      ['bluetooth', 'Bluetooth', 'e14f'],
      ['temperature', 'Temperature', 'e186']
    ];

    const kb = (value) => value == null ? 'Unavailable' : `${(value / 1024 / 1024).toFixed(1)} GB`;
    const onOff = (value) => value == null ? 'Unavailable' : value ? 'On' : 'Off';
    const avg = (arr) => arr.length ? arr.reduce((a, b) => a + b, 0) / arr.length : 0;
    const peak = (arr) => arr.length ? Math.max(...arr) : 0;

    const currentCpu = s.cpu_usage_pct ?? state.sensorHistory.at(-1)?.cpu ?? 0;
    const memUsage = s.mem_total_kb && s.mem_available_kb != null ? (100 * (s.mem_total_kb - s.mem_available_kb) / s.mem_total_kb) : 0;
    const usedMemoryKb = s.mem_total_kb && s.mem_available_kb != null ? s.mem_total_kb - s.mem_available_kb : 0;
    const swapUsage = s.swap_total_kb ? (100 * (s.swap_total_kb - (s.swap_free_kb || 0)) / s.swap_total_kb) : 0;
    const usedSwapKb = s.swap_total_kb ? s.swap_total_kb - (s.swap_free_kb || 0) : 0;

    const totalStorageGb = s.storage_internal_total_kb == null ? (t.storage_internal_total_gb || 0) : s.storage_internal_total_kb / 1024 / 1024;
    const freeStorageGb = s.storage_internal_free_kb == null ? (t.storage_internal_free_gb || 0) : s.storage_internal_free_kb / 1024 / 1024;
    const usedStorageGb = Math.max(0, totalStorageGb - freeStorageGb);
    const storageUsage = totalStorageGb > 0 ? (100 * usedStorageGb / totalStorageGb) : 0;

    const sharedTotalGb = (s.storage_shared_total_kb || 0) / 1024 / 1024;
    const sharedFreeGb = (s.storage_shared_free_kb || 0) / 1024 / 1024;
    const sharedUsedGb = Math.max(0, sharedTotalGb - sharedFreeGb);

    const network = s.network_interfaces || [];
    const temps = s.temperatures || [];
    const processes = s.top_processes || [];
    const sensors = state.sensorInventoryLoaded ? state.sensorInventory : (s.sensor_names || []);

    const hist = state.sensorHistory;
    const cpuHist = hist.map(h => h.cpu ?? 0);
    const ramHist = hist.map(h => h.ram ?? 0);
    const swapHist = hist.map(h => h.swap ?? 0);
    const storageHist = hist.map(h => h.storage ?? 0);
    const rxHist = hist.map(h => h.rxRate ?? 0);
    const txHist = hist.map(h => h.txRate ?? 0);
    const readHist = hist.map(h => h.diskReadRate ?? 0);
    const writeHist = hist.map(h => h.diskWriteRate ?? 0);
    const tcpHist = hist.map(h => h.socketTcp ?? 0);
    const udpHist = hist.map(h => h.socketUdp ?? 0);
    const battHist = hist.map(h => h.battery ?? 0);
    const tempHist = hist.map(h => h.temp ?? 0);

    const latest = hist.at(-1) || {};
    const currRxRate = latest.rxRate ?? 0;
    const currTxRate = latest.txRate ?? 0;
    const netPeakRate = Math.max(1024, ...rxHist, ...txHist);
    const totRx = network.reduce((acc, iface) => acc + (Number(iface.rx_bytes) || 0), 0);
    const totTx = network.reduce((acc, iface) => acc + (Number(iface.tx_bytes) || 0), 0);
    const avgRxRate = avg(rxHist);
    const avgTxRate = avg(txHist);

    const socketTotal = latest.socketTotal || s.socket_total || 0;
    const socketTcp = latest.socketTcp || s.socket_tcp || 0;
    const socketUdp = latest.socketUdp || s.socket_udp || 0;

    const freqGhz = s.cpu_frequencies_mhz?.length ? (Math.max(...s.cpu_frequencies_mhz) / 1000).toFixed(2) : '2.00';
    const minFreqMhz = s.cpu_frequencies_mhz?.length ? Math.min(...s.cpu_frequencies_mhz) : 0;
    const uptimeHours = s.uptime_secs ? Math.floor(s.uptime_secs / 3600) : 0;
    const uptimeMins = s.uptime_secs ? Math.floor((s.uptime_secs % 3600) / 60) : 0;

    // Group Top Apps
    const grouped = new Map();
    for (const p of processes) {
      const name = p.name || 'Unknown process';
      const app = name.split(':')[0];
      const row = grouped.get(app) || { name: app, pids: [], cpu: 0, memory: 0 };
      row.pids.push(p.pid);
      row.cpu += Number(p.cpu_pct) || 0;
      row.memory += Number(p.memory_kb) || 0;
      grouped.set(app, row);
    }
    const apps = [...grouped.values()].filter(p => !state.sensorFilter || p.name.toLowerCase().includes(state.sensorFilter.toLowerCase()));
    apps.sort((a, b) => state.sensorSort === 'memory' ? b.memory - a.memory : state.sensorSort === 'name' ? a.name.localeCompare(b.name) : b.cpu - a.cpu);

    const topAppsTable = () => `
      <section class="card">
        <div class="card-head">
          <div>
            <h2>Top Apps</h2>
            <div class="subtle">Grouped Android processes · current sample · ${grouped.size} apps</div>
          </div>
          <div class="toolbar compact">
            <input class="input" id="sensor-app-filter" value="${escapeHtml(state.sensorFilter)}" placeholder="Filter apps">
            <select class="input sensor-sort" id="sensor-app-sort">
              <option value="cpu" ${state.sensorSort === 'cpu' ? 'selected' : ''}>Sort: CPU</option>
              <option value="memory" ${state.sensorSort === 'memory' ? 'selected' : ''}>Sort: Memory</option>
              <option value="name" ${state.sensorSort === 'name' ? 'selected' : ''}>Sort: Name</option>
            </select>
          </div>
        </div>
        ${apps.length ? `
          <div class="table-wrap">
            <table class="table">
              <thead><tr><th>App / Package</th><th>Processes</th><th>CPU</th><th>Memory</th></tr></thead>
              <tbody>
                ${apps.slice(0, 30).map(p => `
                  <tr>
                    <td class="mono"><b>${escapeHtml(p.name)}</b></td>
                    <td><span class="chip">${p.pids.length} pids</span></td>
                    <td><b>${p.cpu.toFixed(1)}%</b></td>
                    <td>${formatBytes(p.memory * 1024)}</td>
                  </tr>
                `).join('')}
              </tbody>
            </table>
          </div>
        ` : empty('No matching processes', 'Try another search query or refresh.')}
      </section>
    `;

    // 11 Subtab Views matching the Rust app
    const views = {
      overview: () => `
        <div class="sensors-stat-grid">
          ${renderBigStatCard({
            iconCode: 'e186',
            title: 'CPU',
            heroVal: currentCpu.toFixed(1),
            heroUnit: '%',
            subtitle: `${s.cpu_cores || '—'} Cores · ${freqGhz} GHz · ${val(s.cpu_model)}`,
            leftStat: `AVG ${avg(cpuHist).toFixed(0)}%`,
            rightStat: `PEAK ${peak(cpuHist).toFixed(0)}%`,
            data: cpuHist,
            maxVal: 100,
            dottedRatio: 0.12,
            color: 'var(--accent)'
          })}
          ${renderBigStatCard({
            iconCode: 'e085',
            title: 'RAM',
            heroVal: memUsage.toFixed(1),
            heroUnit: '%',
            subtitle: `${kb(usedMemoryKb)} / ${kb(s.mem_total_kb)}`,
            leftStat: `AVG ${avg(ramHist).toFixed(0)}%`,
            rightStat: `PEAK ${peak(ramHist).toFixed(0)}%`,
            data: ramHist,
            maxVal: 100,
            dottedRatio: 0.85,
            color: 'var(--accent)'
          })}
          ${renderBigStatCard({
            iconCode: 'e145',
            title: 'SWAP',
            heroVal: swapUsage.toFixed(1),
            heroUnit: '%',
            subtitle: `${kb(usedSwapKb)} / ${kb(s.swap_total_kb)}`,
            leftStat: `AVG ${avg(swapHist).toFixed(0)}%`,
            rightStat: `PEAK ${peak(swapHist).toFixed(0)}%`,
            data: swapHist,
            maxVal: 100,
            dottedRatio: 0.10,
            color: 'var(--accent)'
          })}
          ${renderBigStatCard({
            iconCode: 'e0c5',
            title: 'STORAGE',
            heroVal: storageUsage.toFixed(1),
            heroUnit: '%',
            subtitle: `${usedStorageGb.toFixed(1)} GB / ${totalStorageGb.toFixed(1)} GB`,
            leftStat: `FREE ${freeStorageGb.toFixed(1)} GB`,
            rightStat: `PEAK ${storageUsage.toFixed(0)}%`,
            data: storageHist,
            maxVal: 100,
            dottedRatio: 0.65,
            color: 'var(--accent)'
          })}
        </div>

        <div class="sensors-overview-split">
          <section class="card overall-speed-card">
            <div class="speed-head-row">
              <div>
                <div class="stat-card-title">OVERALL SPEED</div>
                <div class="subtle" style="margin-top:2px">Interface total · peak ${formatRate(netPeakRate)}</div>
              </div>
              <div class="speed-rates-pill">
                <span class="speed-rate-item down"><span class="rate-arrow">↓</span> Down <b>${formatRate(currRxRate)}</b></span>
                <span class="speed-rate-item up"><span class="rate-arrow">↑</span> Up <b>${formatRate(currTxRate)}</b></span>
              </div>
            </div>
            <div class="speed-chart-box">
              ${renderDualNetworkSvg(txHist, rxHist, netPeakRate, 'var(--accent)')}
            </div>
            <div class="speed-footer-stats">
              <div class="speed-footer-stat-col">
                <small>SENT</small>
                <strong>${formatBytes(totTx)}</strong>
                <span>↑ ${formatRate(currTxRate)} live</span>
              </div>
              <div class="speed-footer-stat-col">
                <small>RECEIVED</small>
                <strong>${formatBytes(totRx)}</strong>
                <span>↓ ${formatRate(currRxRate)} live</span>
              </div>
              <div class="speed-footer-stat-col">
                <small>AVG OVER WINDOW</small>
                <span>↑ ${formatRate(avgTxRate)}</span>
                <span>↓ ${formatRate(avgRxRate)}</span>
              </div>
            </div>
          </section>

          <section class="card conn-stats-card">
            <div class="stat-card-title">CONNECTION STATS</div>
            <div class="conn-stats-hero">
              <strong>${socketTotal}</strong>
              <span>open sockets</span>
            </div>
            <div class="chart-legend" style="margin-bottom:6px">
              <span class="chart-legend-item"><i style="background:var(--accent)"></i>TCP ${socketTcp}</span>
              <span class="chart-legend-item"><i style="background:#94a3b8"></i>UDP ${socketUdp}</span>
            </div>
            <div class="speed-chart-box">
              ${renderSocketsChartSvg(tcpHist, udpHist, 'var(--accent)')}
            </div>
            <div class="conn-interface-chips">
              ${network.slice(0, 3).map(iface => `
                <div class="conn-iface-item">
                  <b>${escapeHtml(iface.name)}</b>
                  <span>↓ ${formatBytes(iface.rx_bytes)} · ↑ ${formatBytes(iface.tx_bytes)}</span>
                </div>
              `).join('')}
            </div>
          </section>
        </div>
      `,

      'top-apps': topAppsTable,

      cpu: () => `
        <section class="card sensor-cpu-head">
          <div class="card-head">
            <div>
              <h2>${escapeHtml(val(s.cpu_model))}</h2>
              <div class="subtle mono">${s.cpu_cores || '—'} cores · ${freqGhz} GHz · ${escapeHtml(val(t.cpu_abi))}</div>
            </div>
            <span class="chip">ANDROID LIVE</span>
          </div>
        </section>

        <div class="sensors-stat-grid">
          ${renderBigStatCard({
            iconCode: 'e181',
            title: 'CPU LOAD',
            heroVal: currentCpu.toFixed(1),
            heroUnit: '%',
            subtitle: `${s.cpu_cores || '—'} cores active`,
            leftStat: `AVG ${avg(cpuHist).toFixed(0)}%`,
            rightStat: `PEAK ${peak(cpuHist).toFixed(0)}%`,
            data: cpuHist,
            maxVal: 100,
            dottedRatio: 0.12,
            color: 'var(--accent)'
          })}
          ${renderBigStatCard({
            iconCode: 'e186',
            title: 'FREQUENCY',
            heroVal: freqGhz,
            heroUnit: 'GHz',
            subtitle: `Min ${(minFreqMhz / 1000).toFixed(2)} GHz`,
            leftStat: 'SCALING',
            rightStat: `${freqGhz} GHz`,
            data: cpuHist,
            maxVal: 100,
            dottedRatio: 0.5,
            color: 'var(--accent)'
          })}
          ${renderBigStatCard({
            iconCode: 'e085',
            title: 'ONLINE CORES',
            heroVal: `${s.cpu_cores || '—'}`,
            heroUnit: 'cores',
            subtitle: 'Kernel /proc/cpuinfo',
            leftStat: 'TOPOLOGY',
            rightStat: 'SMP',
            data: Array(cpuHist.length).fill(s.cpu_cores || 4),
            maxVal: s.cpu_cores || 8,
            dottedRatio: 0.9,
            color: 'var(--accent)'
          })}
          ${renderBigStatCard({
            iconCode: 'e145',
            title: 'UPTIME',
            heroVal: `${uptimeHours}h ${uptimeMins}m`,
            heroUnit: '',
            subtitle: 'Since device boot',
            leftStat: 'KERNEL',
            rightStat: 'ONLINE',
            data: cpuHist,
            maxVal: 100,
            dottedRatio: 0.3,
            color: 'var(--accent)'
          })}
        </div>

        ${renderWaveformChartSvg({
          title: 'CPU Utilization · 60s Window',
          series: [{ label: 'Total CPU', data: cpuHist, color: 'var(--accent)' }],
          height: 140,
          benchmarkVal: 50
        })}

        <section class="card">
          <div class="card-head">
            <h2>Per-Core Telemetry</h2>
            <span class="subtle mono">${s.cpu_cores || 0} physical/logical cores</span>
          </div>
          <div class="cores-grid">
            ${Array.from({ length: s.cpu_cores || 0 }, (_, i) => {
              const coreKey = `cpu${i}`;
              const coreHist = hist.map(h => h.cores?.[coreKey] ?? 0);
              const val = coreHist.at(-1) ?? 0;
              const f = s.cpu_frequencies_mhz?.[i] ? `${(s.cpu_frequencies_mhz[i] / 1000).toFixed(2)} GHz` : 'Online';
              return `
                <div class="core-card">
                  <div class="core-card-head">
                    <span>Core ${i}</span>
                    <b>${val.toFixed(0)}%</b>
                  </div>
                  <div class="core-card-sub">${f}</div>
                  ${renderSparklineSvg(coreHist, 100, 0.5, 'var(--accent)')}
                </div>
              `;
            }).join('')}
          </div>
        </section>

        ${topAppsTable()}
      `,

      ram: () => `
        <div class="sensors-stat-grid">
          ${renderBigStatCard({
            iconCode: 'e085',
            title: 'TOTAL RAM',
            heroVal: `${((s.mem_total_kb || 0) / 1024 / 1024).toFixed(2)}`,
            heroUnit: 'GB',
            subtitle: 'Android kernel memory',
            leftStat: 'TOTAL',
            rightStat: `${kb(s.mem_total_kb)}`,
            data: ramHist,
            maxVal: 100,
            dottedRatio: 0.85,
            color: 'var(--accent)'
          })}
          ${renderBigStatCard({
            iconCode: 'e085',
            title: 'AVAILABLE',
            heroVal: `${((s.mem_available_kb || 0) / 1024 / 1024).toFixed(2)}`,
            heroUnit: 'GB',
            subtitle: `${(100 - memUsage).toFixed(1)}% free memory`,
            leftStat: 'FREE',
            rightStat: `${(100 - memUsage).toFixed(0)}%`,
            data: ramHist.map(v => 100 - v),
            maxVal: 100,
            dottedRatio: 0.15,
            color: '#38bdf8'
          })}
          ${renderBigStatCard({
            iconCode: 'e186',
            title: 'IN USE',
            heroVal: memUsage.toFixed(1),
            heroUnit: '%',
            subtitle: `${kb(usedMemoryKb)} used by apps`,
            leftStat: `AVG ${avg(ramHist).toFixed(0)}%`,
            rightStat: `PEAK ${peak(ramHist).toFixed(0)}%`,
            data: ramHist,
            maxVal: 100,
            dottedRatio: 0.85,
            color: 'var(--accent)'
          })}
          ${renderBigStatCard({
            iconCode: 'e145',
            title: 'SWAP USED',
            heroVal: swapUsage.toFixed(1),
            heroUnit: '%',
            subtitle: `${kb(usedSwapKb)} / ${kb(s.swap_total_kb)}`,
            leftStat: `AVG ${avg(swapHist).toFixed(0)}%`,
            rightStat: `PEAK ${peak(swapHist).toFixed(0)}%`,
            data: swapHist,
            maxVal: 100,
            dottedRatio: 0.1,
            color: '#a78bfa'
          })}
        </div>

        ${renderWaveformChartSvg({
          title: 'Memory Utilization · 60s Window',
          series: [
            { label: 'RAM', data: ramHist, color: 'var(--accent)' },
            { label: 'Swap', data: swapHist, color: '#a78bfa', strokeWidth: 1.8 }
          ],
          height: 150,
          benchmarkVal: 80
        })}

        ${topAppsTable()}
      `,

      gpu: () => `
        <div class="sensors-stat-grid">
          <article class="sensors-stat-card">
            <div class="stat-card-title"><span class="icon">${icon('e0f7')}</span><span>OPENGL RENDERER</span></div>
            <div class="stat-card-hero"><span class="stat-hero-val" style="font-size:18px">${escapeHtml(val(s.gpu_renderer))}</span></div>
            <div class="stat-card-sub">Hardware graphics pipeline</div>
            <div class="stat-card-range"><span>DRIVER</span><span>LIVE</span></div>
            <div class="stat-card-spark">${renderSparklineSvg([100, 100], 100, 0.5, 'var(--accent)')}</div>
          </article>
          <article class="sensors-stat-card">
            <div class="stat-card-title"><span class="icon">${icon('e181')}</span><span>GRAPHICS PLATFORM</span></div>
            <div class="stat-card-hero"><span class="stat-hero-val" style="font-size:18px">${escapeHtml(val(s.gpu_platform))}</span></div>
            <div class="stat-card-sub">API & Vulkan integration</div>
            <div class="stat-card-range"><span>OPENGL ES</span><span>COMPLIANT</span></div>
            <div class="stat-card-spark">${renderSparklineSvg([100, 100], 100, 0.5, 'var(--accent)')}</div>
          </article>
        </div>

        <section class="card">
          <div class="card-head">
            <h2>Graphics Telemetry</h2>
            <span class="chip">ADB LIMITS</span>
          </div>
          <div class="notice">
            Android kernel security limits non-root ADB access to direct GPU clock and memory bus counters. Renderer, platform, and hardware vendor strings are queried through OpenGL ES bindings.
          </div>
        </section>
      `,

      storage: () => `
        <div class="sensors-stat-grid">
          ${renderBigStatCard({
            iconCode: 'e0c5',
            title: 'INTERNAL USED',
            heroVal: storageUsage.toFixed(1),
            heroUnit: '%',
            subtitle: `${freeStorageGb.toFixed(1)} GB free of ${totalStorageGb.toFixed(1)} GB`,
            leftStat: `FREE ${freeStorageGb.toFixed(1)} GB`,
            rightStat: `PEAK ${storageUsage.toFixed(0)}%`,
            data: storageHist,
            maxVal: 100,
            dottedRatio: 0.7,
            color: 'var(--accent)'
          })}
          ${renderBigStatCard({
            iconCode: 'e0c5',
            title: 'SHARED STORAGE',
            heroVal: `${sharedUsedGb.toFixed(1)}`,
            heroUnit: 'GB',
            subtitle: `${sharedFreeGb.toFixed(1)} GB available`,
            leftStat: 'SHARED',
            rightStat: `${sharedTotalGb.toFixed(1)} GB`,
            data: storageHist,
            maxVal: 100,
            dottedRatio: 0.5,
            color: '#38bdf8'
          })}
          ${renderBigStatCard({
            iconCode: 'e186',
            title: 'READ RATE',
            heroVal: formatRate(latest.diskReadRate ?? 0),
            heroUnit: '',
            subtitle: 'Disk read throughput',
            leftStat: `AVG ${formatRate(avg(readHist))}`,
            rightStat: `PEAK ${formatRate(peak(readHist))}`,
            data: readHist,
            maxVal: Math.max(1024, ...readHist),
            dottedRatio: 0.3,
            color: 'var(--accent)'
          })}
          ${renderBigStatCard({
            iconCode: 'e186',
            title: 'WRITE RATE',
            heroVal: formatRate(latest.diskWriteRate ?? 0),
            heroUnit: '',
            subtitle: 'Disk write throughput',
            leftStat: `AVG ${formatRate(avg(writeHist))}`,
            rightStat: `PEAK ${formatRate(peak(writeHist))}`,
            data: writeHist,
            maxVal: Math.max(1024, ...writeHist),
            dottedRatio: 0.3,
            color: '#a78bfa'
          })}
        </div>

        ${renderWaveformChartSvg({
          title: 'Disk Throughput · 60s Window',
          series: [
            { label: 'Read Throughput', data: readHist, color: 'var(--accent)' },
            { label: 'Write Throughput', data: writeHist, color: '#a78bfa' }
          ],
          height: 140,
          suffix: ' B/s'
        })}
      `,

      wifi: () => `
        <div class="sensors-stat-grid">
          ${renderBigStatCard({
            iconCode: 'e1ae',
            title: 'WI-FI RADIO',
            heroVal: onOff(s.wifi_enabled),
            heroUnit: '',
            subtitle: 'Android global Wi-Fi setting',
            leftStat: 'RADIO',
            rightStat: s.wifi_enabled ? 'ONLINE' : 'OFFLINE',
            data: [s.wifi_enabled ? 1 : 0, s.wifi_enabled ? 1 : 0],
            maxVal: 1,
            dottedRatio: 0.5,
            color: 'var(--accent)'
          })}
          ${renderBigStatCard({
            iconCode: 'e119',
            title: 'INTERFACES',
            heroVal: `${network.length}`,
            heroUnit: 'active',
            subtitle: 'Exposed via /proc/net/dev',
            leftStat: 'IFACES',
            rightStat: 'COUNT',
            data: Array(rxHist.length).fill(network.length),
            maxVal: network.length || 1,
            dottedRatio: 0.9,
            color: '#38bdf8'
          })}
          ${renderBigStatCard({
            iconCode: 'e186',
            title: 'CURRENT RX',
            heroVal: formatRate(currRxRate),
            heroUnit: '',
            subtitle: 'Live network downlink',
            leftStat: `AVG ${formatRate(avgRxRate)}`,
            rightStat: `PEAK ${formatRate(peak(rxHist))}`,
            data: rxHist,
            maxVal: Math.max(1024, ...rxHist),
            dottedRatio: 0.3,
            color: '#38bdf8'
          })}
          ${renderBigStatCard({
            iconCode: 'e186',
            title: 'CURRENT TX',
            heroVal: formatRate(currTxRate),
            heroUnit: '',
            subtitle: 'Live network uplink',
            leftStat: `AVG ${formatRate(avgTxRate)}`,
            rightStat: `PEAK ${formatRate(peak(txHist))}`,
            data: txHist,
            maxVal: Math.max(1024, ...txHist),
            dottedRatio: 0.3,
            color: 'var(--accent)'
          })}
        </div>

        ${renderWaveformChartSvg({
          title: 'Network Throughput · 60s Window',
          series: [
            { label: 'Received (Downlink)', data: rxHist, color: '#38bdf8' },
            { label: 'Transmitted (Uplink)', data: txHist, color: 'var(--accent)' }
          ],
          height: 140,
          suffix: ' B/s'
        })}

        <section class="card">
          <div class="card-head">
            <h2>Network Interfaces</h2>
            <span class="subtle mono">${network.length} interfaces reported</span>
          </div>
          ${network.length ? `
            <div class="table-wrap">
              <table class="table">
                <thead><tr><th>Interface</th><th>Received</th><th>Sent</th><th>Total</th></tr></thead>
                <tbody>
                  ${network.map(n => `
                    <tr>
                      <td class="mono"><b>${escapeHtml(n.name)}</b></td>
                      <td>↓ ${formatBytes(n.rx_bytes)}</td>
                      <td>↑ ${formatBytes(n.tx_bytes)}</td>
                      <td><b>${formatBytes(n.rx_bytes + n.tx_bytes)}</b></td>
                    </tr>
                  `).join('')}
                </tbody>
              </table>
            </div>
          ` : empty('No interfaces available', 'The device did not expose /proc/net/dev to ADB.')}
        </section>
      `,

      'data-usage': () => `
        ${renderWaveformChartSvg({
          title: 'Cumulative Network Traffic · 60s Trend',
          series: [
            { label: 'Cumulative Downlink', data: hist.map(h => h.rx ?? 0), color: '#38bdf8' },
            { label: 'Cumulative Uplink', data: hist.map(h => h.tx ?? 0), color: 'var(--accent)' }
          ],
          height: 140,
          suffix: ' bytes'
        })}

        <section class="card">
          <div class="card-head">
            <div>
              <h2>Interface Data Counters</h2>
              <div class="subtle">Cumulative bytes reported by Android kernel since device boot</div>
            </div>
          </div>
          ${network.length ? `
            <div class="table-wrap">
              <table class="table">
                <thead><tr><th>Interface</th><th>Received</th><th>Transmitted</th><th>Total</th></tr></thead>
                <tbody>
                  ${network.map(n => `
                    <tr>
                      <td class="mono"><b>${escapeHtml(n.name)}</b></td>
                      <td>↓ ${formatBytes(n.rx_bytes)}</td>
                      <td>↑ ${formatBytes(n.tx_bytes)}</td>
                      <td><b>${formatBytes(n.rx_bytes + n.tx_bytes)}</b></td>
                    </tr>
                  `).join('')}
                </tbody>
              </table>
            </div>
          ` : empty('No data counters', 'Kernel byte counters unavailable.')}
        </section>
      `,

      power: () => `
        <div class="sensors-stat-grid">
          ${renderBigStatCard({
            iconCode: 'e145',
            title: 'BATTERY LEVEL',
            heroVal: val(s.battery_pct, '%'),
            heroUnit: '',
            subtitle: val(s.battery_status),
            leftStat: `PEAK ${peak(battHist)}%`,
            rightStat: `AVG ${avg(battHist).toFixed(0)}%`,
            data: battHist,
            maxVal: 100,
            dottedRatio: 0.5,
            color: 'var(--accent)'
          })}
          ${renderBigStatCard({
            iconCode: 'e186',
            title: 'TEMPERATURE',
            heroVal: val(s.battery_temp_c, ' °C'),
            heroUnit: '',
            subtitle: s.battery_health ? `Health code: ${s.battery_health}` : 'Battery thermal zone',
            leftStat: 'THERMAL',
            rightStat: `${val(s.battery_temp_c, '°C')}`,
            data: tempHist,
            maxVal: Math.max(60, ...tempHist),
            dottedRatio: 0.4,
            color: '#fbbf24'
          })}
          ${renderBigStatCard({
            iconCode: 'e145',
            title: 'VOLTAGE',
            heroVal: s.battery_voltage_mv ? `${(s.battery_voltage_mv / 1000).toFixed(2)}` : 'Unavailable',
            heroUnit: s.battery_voltage_mv ? 'V' : '',
            subtitle: `Plugged: ${val(s.battery_plugged)}`,
            leftStat: 'CHARGER',
            rightStat: val(s.battery_plugged),
            data: battHist,
            maxVal: 100,
            dottedRatio: 0.5,
            color: 'var(--accent)'
          })}
          ${renderBigStatCard({
            iconCode: 'e101',
            title: 'UPTIME',
            heroVal: `${uptimeHours}h ${uptimeMins}m`,
            heroUnit: '',
            subtitle: 'Since last boot',
            leftStat: 'POWER',
            rightStat: 'ACTIVE',
            data: battHist,
            maxVal: 100,
            dottedRatio: 0.3,
            color: 'var(--accent)'
          })}
        </div>

        ${renderWaveformChartSvg({
          title: 'Battery Level & Thermal History · 60s Window',
          series: [
            { label: 'Charge %', data: battHist, color: 'var(--accent)' },
            { label: 'Temperature (°C)', data: tempHist, color: '#fbbf24' }
          ],
          height: 140,
          benchmarkVal: 50
        })}
      `,

      bluetooth: () => `
        <div class="sensors-stat-grid">
          ${renderBigStatCard({
            iconCode: 'e14f',
            title: 'BLUETOOTH RADIO',
            heroVal: onOff(s.bluetooth_enabled),
            heroUnit: '',
            subtitle: 'Android global Bluetooth setting',
            leftStat: 'RADIO',
            rightStat: s.bluetooth_enabled ? 'ONLINE' : 'OFFLINE',
            data: [s.bluetooth_enabled ? 1 : 0, s.bluetooth_enabled ? 1 : 0],
            maxVal: 1,
            dottedRatio: 0.5,
            color: 'var(--accent)'
          })}
          ${renderBigStatCard({
            iconCode: 'e14f',
            title: 'ADAPTER STATE',
            heroVal: s.bluetooth_enabled ? 'Enabled' : 'Disabled',
            heroUnit: '',
            subtitle: 'Host Bluetooth controller',
            leftStat: 'STATE',
            rightStat: s.bluetooth_enabled ? 'ACTIVE' : 'IDLE',
            data: [s.bluetooth_enabled ? 1 : 0, s.bluetooth_enabled ? 1 : 0],
            maxVal: 1,
            dottedRatio: 0.5,
            color: 'var(--accent)'
          })}
        </div>

        <section class="card">
          <div class="card-head">
            <h2>Bluetooth Status</h2>
            <span class="chip">SETTINGS INTEGRATION</span>
          </div>
          <div class="notice">
            Bluetooth telemetry queries Android global wireless settings. Connected peripheral telemetry requires active Bluetooth pairing services.
          </div>
        </section>
      `,

      temperature: () => {
        const maxTemp = temps.length ? Math.max(...temps.map(x => Number(x.temp_c)).filter(Number.isFinite)) : (s.battery_temp_c || 0);
        return `
          <div class="sensors-stat-grid">
            ${renderBigStatCard({
              iconCode: 'e186',
              title: 'HIGHEST TEMP',
              heroVal: `${maxTemp.toFixed(1)}`,
              heroUnit: '°C',
              subtitle: 'Warmest kernel thermal zone',
              leftStat: `AVG ${avg(tempHist).toFixed(0)}°C`,
              rightStat: `PEAK ${peak(tempHist).toFixed(0)}°C`,
              data: tempHist,
              maxVal: Math.max(70, ...tempHist),
              dottedRatio: 0.45,
              color: maxTemp >= 60 ? '#f87171' : maxTemp >= 45 ? '#fbbf24' : 'var(--accent)'
            })}
            ${renderBigStatCard({
              iconCode: 'e145',
              title: 'BATTERY TEMP',
              heroVal: val(s.battery_temp_c, ' °C'),
              heroUnit: '',
              subtitle: 'Integrated battery sensor',
              leftStat: 'BATTERY',
              rightStat: `${val(s.battery_temp_c, '°C')}`,
              data: tempHist,
              maxVal: Math.max(60, ...tempHist),
              dottedRatio: 0.4,
              color: '#38bdf8'
            })}
            ${renderBigStatCard({
              iconCode: 'e186',
              title: 'THERMAL ZONES',
              heroVal: `${temps.length}`,
              heroUnit: 'zones',
              subtitle: 'Accessible /sys/class/thermal',
              leftStat: 'KERNEL',
              rightStat: `${temps.length} ZONES`,
              data: Array(tempHist.length).fill(temps.length),
              maxVal: temps.length || 1,
              dottedRatio: 0.9,
              color: 'var(--accent)'
            })}
            ${renderBigStatCard({
              iconCode: 'e085',
              title: 'REGISTERED SENSORS',
              heroVal: `${sensors.length}`,
              heroUnit: 'sensors',
              subtitle: 'Dumpsys sensorservice inventory',
              leftStat: 'HARDWARE',
              rightStat: `${sensors.length} REPORTERS`,
              data: Array(tempHist.length).fill(sensors.length),
              maxVal: sensors.length || 1,
              dottedRatio: 0.9,
              color: '#a78bfa'
            })}
          </div>

          ${renderWaveformChartSvg({
            title: 'Thermal History · 60s Window',
            series: [{ label: 'Thermal Zone History', data: tempHist, color: '#fbbf24' }],
            height: 140,
            suffix: ' °C',
            benchmarkVal: 45
          })}

          <section class="card">
            <div class="card-head">
              <div>
                <h2>Thermal Zones</h2>
                <div class="subtle">Read from accessible Android kernel thermal zones</div>
              </div>
            </div>
            ${temps.length ? `
              <div class="thermal-grid">
                ${temps.map(t => {
                  const val = Number(t.temp_c) || 0;
                  const status = val >= 60 ? 'hot' : val >= 45 ? 'warm' : 'normal';
                  const statusText = val >= 60 ? 'HIGH' : val >= 45 ? 'WARM' : 'NORMAL';
                  return `
                    <div class="thermal-zone-card">
                      <div class="stat-card-title">${escapeHtml(t.name)}</div>
                      <div class="metric-value">${val.toFixed(1)} °C</div>
                      <div><span class="thermal-badge ${status}">${statusText}</span></div>
                    </div>
                  `;
                }).join('')}
              </div>
            ` : empty('Thermal readings unavailable', 'Android did not expose current thermal zone values.')}
          </section>

          <section class="card">
            <div class="card-head">
              <h2>Registered Sensors</h2>
              <span class="chip">${sensors.length} reported</span>
            </div>
            ${sensors.length ? `
              <div class="sensor-tags">
                ${sensors.map(name => `<span class="chip">${escapeHtml(name)}</span>`).join('')}
              </div>
            ` : state.sensorInventoryLoading ? empty('Loading sensor inventory', 'Android sensor metadata is being read once for this device.') : empty('Sensor inventory unavailable', 'The device does not expose its sensor list through dumpsys sensorservice.')}
          </section>
        `;
      }
    };

    const activeView = state.sensorsView in views ? views[state.sensorsView]() : views.overview();

    content.innerHTML = `
      <div class="sensors-shell">
        <aside class="sensors-sidebar">
          <div class="sensors-sidebar-title">
            <span>TELEMETRY</span>
            <span class="chip compact">ADB</span>
          </div>
          <nav class="sensors-subnav" aria-label="Sensor subtabs">
            ${subtabs.map(([id, label, iconCode]) => `
              <button class="sensors-subtab-btn ${state.sensorsView === id ? 'active' : ''}" data-sensor-view="${id}">
                <span class="icon">${icon(iconCode)}</span>
                <span>${escapeHtml(label)}</span>
              </button>
            `).join('')}
          </nav>
          <div class="sensors-sidebar-footer">
            <div class="live-pulse-badge">
              <div class="live-pulse-dot" style="${state.sensorsCadence === 0 ? 'background:var(--dim);box-shadow:none;animation:none;' : ''}"></div>
              <span>${state.sensorsCadence === 0 ? 'PAUSED' : 'LIVE 60S'}</span>
            </div>
            <label class="sensor-cadence-label" for="sensor-cadence">Cadence</label>
            <select class="input sensor-cadence-select" id="sensor-cadence">
              <option value="1000" ${state.sensorsCadence === 1000 ? 'selected' : ''}>1s (Fast)</option>
              <option value="2000" ${state.sensorsCadence === 2000 ? 'selected' : ''}>2s (Standard)</option>
              <option value="5000" ${state.sensorsCadence === 5000 ? 'selected' : ''}>5s (Low power)</option>
              <option value="0" ${state.sensorsCadence === 0 ? 'selected' : ''}>Paused</option>
            </select>
          </div>
        </aside>
        <section class="sensors-main">
          ${activeView}
        </section>
      </div>
    `;

    // Event wiring
    const filter = $('#sensor-app-filter');
    const sort = $('#sensor-app-sort');
    const cadence = $('#sensor-cadence');

    if (filter) {
      filter.oninput = () => {
        state.sensorFilter = filter.value;
        renderSensors();
        const next = $('#sensor-app-filter');
        if (next) {
          next.focus();
          next.setSelectionRange(next.value.length, next.value.length);
        }
      };
    }
    if (sort) {
      sort.onchange = () => {
        state.sensorSort = sort.value;
        renderSensors();
      };
    }
    if (cadence) {
      cadence.onchange = () => {
        state.sensorsCadence = Number(cadence.value);
        clearTimeout(state.sensorTimer);
        if (state.sensorsCadence > 0) {
          state.sensorTimer = setTimeout(pollSensors, state.sensorsCadence);
        }
        renderSensors();
      };
    }

    if (state.sensorsView === 'temperature' && !state.sensorInventoryLoaded && !state.sensorInventoryLoading && state.serial) {
      state.sensorInventoryLoading = true;
      const serial = state.serial;
      invokeOrFail('adb_data', { serial, section: 'sensor_inventory', path: null }).then(names => {
        if (state.serial === serial) {
          state.sensorInventory = names;
          state.sensorInventoryLoaded = true;
        }
      }).catch(error => toast(String(error), true)).finally(() => {
        state.sensorInventoryLoading = false;
        if (state.tab === 'sensors' && state.sensorsView === 'temperature' && state.serial === serial) {
          renderSensors();
        }
      });
    }

    clearTimeout(state.sensorTimer);
    if (state.tab === 'sensors' && !document.hidden && state.sensorsCadence > 0) {
      state.sensorTimer = setTimeout(pollSensors, state.sensorsCadence);
    }
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
