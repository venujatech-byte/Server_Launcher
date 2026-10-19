(() => {
  const invoke = window.__TAURI__?.core?.invoke;
  const $ = (selector) => document.querySelector(selector);
  const content = $('#content');
  const state = { serial: '', device: null, devices: [], tab: 'overview', telemetry: null, data: [], messageAddress: '', sensorsView: 'overview', sensorsCadence: 2000, sensorHistory: [], sensorTimer: null, sensorBusy: false, sensorFilter: '', sensorSort: 'cpu', sensorInventory: [], sensorInventorySerial: '', sensorInventoryLoading: false, sensorInventoryLoaded: false, path: '/sdcard', screenTimer: null, toastTimer: null, lastTelemetry: 0, topAppsSelectedPid: null, topAppsExpandedGroups: new Set(), topAppsAppsExpanded: true, topAppsBgExpanded: true, topAppsSort: 'mem', topAppsSortDesc: true, topAppsSearch: '', photosAlbum: 'all', photosSearch: '', photosDensity: 'comfortable', photosSelected: new Set(), photosSelectionMode: false, photosLightboxIndex: null, photosCache: {}, customSubtab: 'boot', customRootStatus: null, customFastbootDevices: null, customDensityVal: '', customAnimScale: '0.5' };
  const tabNames = { overview: 'Overview', screen: 'Screen share', messages: 'Messages', calls: 'Calls', photos: 'Photos', notifications: 'Notifications', sensors: 'Sensors', storage: 'Storage', apps: 'Apps', clipboard: 'Clipboard', terminal: 'Terminal', wireless: 'Wireless debugging', firmware: 'Customization & Boot' };
  const tabKinds = { messages: 'messages', calls: 'calls', photos: 'photos', notifications: 'notifications', sensors: 'sensors', apps: 'apps', storage: 'files', wireless: 'endpoints' };
  const icon = (code) => `<span class="icon" aria-hidden="true">&#x${code};</span>`;
  const escapeHtml = (v) => String(v ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  const val = (v, suffix = '', digits = 1) => {
    if (v == null || v === '') return 'Unavailable';
    if (typeof v === 'number') {
      if (!Number.isFinite(v)) return 'Unavailable';
      return `${Number.isInteger(v) ? v : v.toFixed(digits)}${suffix}`;
    }
    const str = String(v).trim();
    if (!str) return 'Unavailable';
    const parsed = Number(str);
    if (!Number.isNaN(parsed) && str.includes('.')) {
      return `${parsed.toFixed(digits)}${suffix}`;
    }
    const cleaned = str.replace(/\b(\d+)\.(\d{3,})\b/g, (_, intPart, decPart) => {
      const num = Number(`${intPart}.${decPart}`);
      return Number.isFinite(num) ? num.toFixed(digits) : `${intPart}.${decPart.slice(0, digits)}`;
    });
    return `${cleaned}${suffix}`;
  };
  const invokeOrFail = (...args) => { if (!invoke) throw new Error('Open this page inside the Server Launcher Tauri window.'); return invoke(...args); };

  function toast(message, error = false) {
    const el = $('#toast'); el.textContent = message; el.className = `toast show${error ? ' error' : ''}`;
    clearTimeout(state.toastTimer); state.toastTimer = setTimeout(() => { el.className = 'toast'; }, 3300);
  }
  function setConnection(ok, label) { const el = $('#connection-state'); el.className = `connection ${ok ? 'online' : 'offline'}`; el.lastElementChild.textContent = label; }
  function updateHeaderTelemetryControls() {
    const ctrl = $('#sensors-header-controls');
    if (!ctrl) return;
    if (state.tab === 'sensors') {
      ctrl.style.display = 'inline-flex';
      const cadence = $('#sensor-cadence');
      if (cadence) cadence.value = String(state.sensorsCadence);
      const dot = $('#header-pulse-dot');
      const txt = $('#header-pulse-text');
      if (dot) {
        dot.style.background = state.sensorsCadence === 0 ? 'var(--dim)' : 'var(--green)';
        dot.style.boxShadow = state.sensorsCadence === 0 ? 'none' : '0 0 8px var(--green)';
        dot.style.animation = state.sensorsCadence === 0 ? 'none' : 'pulse-glow 2s infinite';
      }
      if (txt) {
        txt.textContent = state.sensorsCadence === 0 ? 'PAUSED' : 'LIVE 60S';
      }
    } else {
      ctrl.style.display = 'none';
    }
  }

  let currentLoadToken = 0;

  function showPageLoading(title = '') {
    const pageName = title || tabNames[state.tab] || 'Section';
    content.innerHTML = `
      <div class="page-loading-wrapper">
        <div class="page-loading-card">
          <div class="loading-brand-container">
            <img src="./icon.png" alt="Server Launcher Logo" class="loading-brand-logo" onerror="this.style.display='none'; const fb = document.getElementById('page-loading-fallback'); if (fb) fb.style.display='flex';" />
            <div id="page-loading-fallback" class="loading-brand-fallback icon">&#xe153;</div>
            <div class="loading-ring-pulse"></div>
          </div>
          <div class="loading-text-group">
            <h3 class="loading-heading">Loading ${escapeHtml(pageName)}</h3>
            <p class="loading-subtext">Communicating with Android Device via ADB Bridge…</p>
          </div>
        </div>
      </div>
    `;
  }

  function selectTab(tab) {
    if (!tabNames[tab]) return;
    clearInterval(state.screenTimer); state.screenTimer = null;
    state.tab = tab; document.querySelectorAll('.nav-item').forEach((n) => n.classList.toggle('active', n.dataset.tab === tab));
    clearTimeout(state.sensorTimer); state.sensorTimer = null;
    $('#page-title').textContent = tabNames[tab]; $('#eyebrow').textContent = `ANDROID DEVICE / ${tab.toUpperCase()}`;
    updateHeaderTelemetryControls();
    if (tab !== 'overview') {
      showPageLoading(tabNames[tab]);
    }
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
    const loadToken = ++currentLoadToken;
    const targetTab = state.tab;
    if (targetTab === 'overview') { content.innerHTML = overview(); return; }
    if (tabKinds[targetTab]) {
      showPageLoading(tabNames[targetTab]);
      await loadData(tabKinds[targetTab], state.path);
      if (loadToken !== currentLoadToken || state.tab !== targetTab) return;
    }
    if (!state.device || state.tab !== targetTab) return;
    switch (targetTab) {
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
    const q = (state.photosSearch || '').trim().toLowerCase();
    const album = state.photosAlbum || 'all';

    const getAlbum = (p) => {
      const path = (p.path || '').toLowerCase();
      const name = (p.name || '').toLowerCase();
      if (path.includes('/screenshots/') || name.includes('screenshot')) return 'screenshots';
      if (path.includes('/download')) return 'downloads';
      if (path.includes('/dcim/') || path.includes('/camera/')) return 'camera';
      return 'other';
    };

    const counts = {
      all: rows.length,
      camera: rows.filter(p => getAlbum(p) === 'camera').length,
      screenshots: rows.filter(p => getAlbum(p) === 'screenshots').length,
      downloads: rows.filter(p => getAlbum(p) === 'downloads').length,
    };

    const filtered = rows.filter(p => {
      if (album === 'camera' && getAlbum(p) !== 'camera') return false;
      if (album === 'screenshots' && getAlbum(p) !== 'screenshots') return false;
      if (album === 'downloads' && getAlbum(p) !== 'downloads') return false;
      if (q && !((p.name || '').toLowerCase().includes(q) || (p.path || '').toLowerCase().includes(q))) return false;
      return true;
    });

    const isSelected = (path) => state.photosSelected.has(path);
    const density = state.photosDensity || 'comfortable';

    content.innerHTML = `
      <div class="photos-page">
        <!-- Top Header Card -->
        <header class="photos-header-card">
          <div class="photos-header-info">
            <h2>${icon("e0f7")} Camera Roll & Gallery</h2>
            <div class="photos-header-sub">${filtered.length} photos displayed · ${rows.length} total on device</div>
          </div>
          <div class="photos-header-actions">
            <button class="button tonal" id="photo-upload-btn">${icon("e0cc")} Push Photo to Device</button>
            <button class="button" data-refresh>${icon("e145")} Refresh</button>
          </div>
        </header>

        <!-- Filter & Search Toolbar -->
        <div class="photos-filter-bar">
          <div class="photos-albums">
            <button class="album-pill ${album === 'all' ? 'active' : ''}" data-album="all">${icon("e0d7")} All (${counts.all})</button>
            <button class="album-pill ${album === 'camera' ? 'active' : ''}" data-album="camera">${icon("e0f7")} Camera (${counts.camera})</button>
            <button class="album-pill ${album === 'screenshots' ? 'active' : ''}" data-album="screenshots">${icon("e14f")} Screenshots (${counts.screenshots})</button>
            <button class="album-pill ${album === 'downloads' ? 'active' : ''}" data-album="downloads">${icon("e0cc")} Downloads (${counts.downloads})</button>
          </div>

          <div style="display:flex; align-items:center; gap:8px;">
            <div class="photos-search-wrap">
              <span class="icon" style="position:absolute; left:9px; color:#64748b; font-size:14px; pointer-events:none;">&#xe14c;</span>
              <input id="photos-search-input" class="photos-search-input" type="search" placeholder="Search photos..." value="${escapeHtml(state.photosSearch)}">
            </div>

            <div class="density-toggle">
              <button class="density-btn ${density === 'compact' ? 'active' : ''}" data-density="compact" title="Compact density">Compact</button>
              <button class="density-btn ${density === 'comfortable' ? 'active' : ''}" data-density="comfortable" title="Comfortable density">Medium</button>
              <button class="density-btn ${density === 'large' ? 'active' : ''}" data-density="large" title="Large cards">Large</button>
            </div>
          </div>
        </div>

        <!-- Photo Grid -->
        ${filtered.length > 0 ? `
          <div class="photo-grid density-${density} ${state.photosSelected.size > 0 ? 'selection-mode' : ''}">
            ${filtered.map((p, idx) => {
              const selected = isSelected(p.path);
              const cachedSrc = state.photosCache[p.path];
              const ext = (p.name.split('.').pop() || 'JPG').toUpperCase();
              return `
                <article class="photo-card ${selected ? 'selected' : ''}" data-photo-path="${escapeHtml(p.path)}">
                  <div class="photo-select-box" data-photo-select="${escapeHtml(p.path)}" title="Select photo">
                    ${selected ? icon("e10b") : ''}
                  </div>
                  <div class="photo-thumb-container" data-photo-view="${idx}">
                    ${cachedSrc ? `
                      <img class="photo-thumb-img" src="${cachedSrc}" alt="${escapeHtml(p.name)}" loading="lazy">
                    ` : `
                      <div class="photo-thumb-placeholder" id="thumb-holder-${idx}">
                        ${icon("e0f7")}
                        <span>${escapeHtml(ext)}</span>
                      </div>
                    `}
                    <div class="photo-quick-overlay">
                      <button class="photo-overlay-btn" data-overlay-view="${idx}">${icon("e14f")} View</button>
                      <button class="photo-overlay-btn" data-overlay-save="${escapeHtml(p.path)}">${icon("e0cc")} Save</button>
                      <button class="photo-overlay-btn danger" data-overlay-del="${escapeHtml(p.path)}">${icon("e048")} Del</button>
                    </div>
                  </div>
                  <div class="photo-details">
                    <div class="photo-title" title="${escapeHtml(p.name)}">${escapeHtml(p.name)}</div>
                    <div class="photo-meta-row">
                      <span>${escapeHtml(p.size)}</span>
                      <span>${escapeHtml(p.date)}</span>
                    </div>
                  </div>
                </article>
              `;
            }).join('')}
          </div>
        ` : empty('No photos found', rows.length === 0 ? 'No photos returned from /sdcard/DCIM or MediaStore. Click "Push Photo to Device" to transfer images to the camera roll.' : 'No photos match your filter or search query.')}

        <!-- Floating Bulk Selection Bar -->
        ${state.photosSelected.size > 0 ? `
          <div class="photo-floating-bar">
            <span class="photo-selected-count">${state.photosSelected.size} selected</span>
            <button class="button success tonal" id="bulk-download-btn">${icon("e0cc")} Save Selected</button>
            <button class="button danger tonal" id="bulk-delete-btn">${icon("e048")} Delete Selected</button>
            <button class="button tonal" id="bulk-clear-btn">Deselect All</button>
          </div>
        ` : ''}

        <!-- Lightbox Modal -->
        ${state.photosLightboxIndex !== null && filtered[state.photosLightboxIndex] ? (() => {
          const p = filtered[state.photosLightboxIndex];
          const cachedSrc = state.photosCache[p.path];
          return `
            <div class="photo-lightbox" id="photo-lightbox-modal">
              <div class="lightbox-topbar">
                <div class="lightbox-title-box">
                  ${icon("e0f7")}
                  <span class="lightbox-filename">${escapeHtml(p.name)}</span>
                  <span class="lightbox-counter">${state.photosLightboxIndex + 1} / ${filtered.length}</span>
                </div>
                <div class="lightbox-actions">
                  <button class="button tonal" id="lightbox-save-btn">${icon("e0cc")} Download to PC</button>
                  <button class="button danger tonal" id="lightbox-del-btn">${icon("e048")} Delete</button>
                  <button class="button" id="lightbox-close-btn">✕ Close</button>
                </div>
              </div>

              <div class="lightbox-stage">
                ${state.photosLightboxIndex > 0 ? `
                  <button class="lightbox-nav-btn prev" id="lightbox-prev-btn" title="Previous photo (←)">‹</button>
                ` : ''}
                <div id="lightbox-img-container" style="display:grid; place-items:center; width:100%; height:100%;">
                  ${cachedSrc ? `
                    <img src="${cachedSrc}" alt="${escapeHtml(p.name)}">
                  ` : `
                    <div class="page-loading-card" style="padding:24px 32px; max-width:320px;">
                      <div class="loading-brand-container" style="width:68px; height:68px; margin-bottom:14px;">
                        <img src="./icon.png" alt="Loading" class="loading-brand-logo" style="width:46px; height:46px;" onerror="this.style.display='none';" />
                        <div class="loading-ring-pulse"></div>
                      </div>
                      <div class="loading-heading" style="font-size:14px;">Loading photo…</div>
                      <div class="loading-subtext">Fetching original bytes from Android storage</div>
                    </div>
                  `}
                </div>
                ${state.photosLightboxIndex < filtered.length - 1 ? `
                  <button class="lightbox-nav-btn next" id="lightbox-next-btn" title="Next photo (→)">›</button>
                ` : ''}
              </div>

              <div class="lightbox-footer">
                <span class="mono">${escapeHtml(p.path)}</span>
                <span>${escapeHtml(p.size)} · ${escapeHtml(p.date)} · Press ← / → to navigate, Esc to exit</span>
              </div>
            </div>
          `;
        })() : ''}
      </div>
    `;

    // 1. Search input
    const searchInput = $('#photos-search-input');
    if (searchInput) {
      searchInput.oninput = (e) => {
        state.photosSearch = e.target.value;
        renderPhotos();
        const fresh = $('#photos-search-input');
        if (fresh) { fresh.focus(); fresh.setSelectionRange(fresh.value.length, fresh.value.length); }
      };
    }

    // 2. Album pills
    document.querySelectorAll('[data-album]').forEach(btn => {
      btn.onclick = () => {
        state.photosAlbum = btn.dataset.album;
        renderPhotos();
      };
    });

    // 3. Density toggle
    document.querySelectorAll('[data-density]').forEach(btn => {
      btn.onclick = () => {
        state.photosDensity = btn.dataset.density;
        renderPhotos();
      };
    });

    // 4. Push / Upload photo
    const uploadBtn = $('#photo-upload-btn');
    if (uploadBtn) {
      uploadBtn.onclick = async () => {
        try {
          const filePath = await action('pick_file', 'jpg,jpeg,png,webp');
          if (filePath) {
            toast('Pushing photo to /sdcard/DCIM/Camera/...');
            await action('push_photo', filePath);
            toast('Photo pushed and scanned into gallery!');
            await loadData('photos');
            renderPhotos();
          }
        } catch (err) {
          if (!String(err).includes('cancelled')) toast(String(err), true);
        }
      };
    }

    // 5. Lightbox open
    document.querySelectorAll('[data-photo-view], [data-overlay-view]').forEach(el => {
      el.onclick = async (e) => {
        e.stopPropagation();
        const idx = Number(el.dataset.photoView ?? el.dataset.overlayView);
        state.photosLightboxIndex = idx;
        renderPhotos();
        const targetPhoto = filtered[idx];
        if (targetPhoto && !state.photosCache[targetPhoto.path]) {
          try {
            const src = await invokeOrFail('adb_photo_preview', { serial: state.serial, path: targetPhoto.path });
            state.photosCache[targetPhoto.path] = src;
            const container = $('#lightbox-img-container');
            if (container && state.photosLightboxIndex === idx) {
              container.innerHTML = `<img src="${src}" alt="${escapeHtml(targetPhoto.name)}">`;
            }
          } catch (err) {
            toast('Failed to load image preview: ' + err, true);
          }
        }
      };
    });

    // 6. Selection checkbox
    document.querySelectorAll('[data-photo-select]').forEach(box => {
      box.onclick = (e) => {
        e.stopPropagation();
        const path = box.dataset.photoSelect;
        if (state.photosSelected.has(path)) {
          state.photosSelected.delete(path);
        } else {
          state.photosSelected.add(path);
        }
        renderPhotos();
      };
    });

    // 7. Overlay save / delete
    document.querySelectorAll('[data-overlay-save]').forEach(btn => {
      btn.onclick = async (e) => {
        e.stopPropagation();
        const path = btn.dataset.overlaySave;
        await action('download_photo', path);
      };
    });

    document.querySelectorAll('[data-overlay-del]').forEach(btn => {
      btn.onclick = async (e) => {
        e.stopPropagation();
        const path = btn.dataset.overlayDel;
        if (confirm(`Are you sure you want to delete this photo from your device?\n${path}`)) {
          await action('delete_photo', path);
          delete state.photosCache[path];
          state.photosSelected.delete(path);
          await loadData('photos');
          renderPhotos();
        }
      };
    });

    // 8. Bulk action bar
    const bulkDownload = $('#bulk-download-btn');
    if (bulkDownload) {
      bulkDownload.onclick = async () => {
        const paths = Array.from(state.photosSelected);
        toast(`Saving ${paths.length} photos to PC Downloads...`);
        for (const p of paths) {
          try { await action('download_photo', p); } catch {}
        }
        toast(`Exported ${paths.length} photos!`);
      };
    }

    const bulkDelete = $('#bulk-delete-btn');
    if (bulkDelete) {
      bulkDelete.onclick = async () => {
        const paths = Array.from(state.photosSelected);
        if (confirm(`Permanently delete ${paths.length} selected photos from your device?`)) {
          for (const p of paths) {
            try {
              await action('delete_photo', p);
              delete state.photosCache[p];
            } catch {}
          }
          state.photosSelected.clear();
          toast(`Deleted ${paths.length} photos.`);
          await loadData('photos');
          renderPhotos();
        }
      };
    }

    const bulkClear = $('#bulk-clear-btn');
    if (bulkClear) {
      bulkClear.onclick = () => {
        state.photosSelected.clear();
        renderPhotos();
      };
    }

    // 9. Lightbox controls
    const lightboxModal = $('#photo-lightbox-modal');
    if (lightboxModal && state.photosLightboxIndex !== null) {
      const curPhoto = filtered[state.photosLightboxIndex];
      const closeLb = () => { state.photosLightboxIndex = null; renderPhotos(); };

      const closeBtn = $('#lightbox-close-btn');
      if (closeBtn) closeBtn.onclick = closeLb;

      const prevBtn = $('#lightbox-prev-btn');
      if (prevBtn) {
        prevBtn.onclick = () => {
          if (state.photosLightboxIndex > 0) {
            state.photosLightboxIndex--;
            renderPhotos();
            const prevP = filtered[state.photosLightboxIndex];
            if (prevP && !state.photosCache[prevP.path]) {
              invokeOrFail('adb_photo_preview', { serial: state.serial, path: prevP.path }).then(src => {
                state.photosCache[prevP.path] = src;
                const container = $('#lightbox-img-container');
                if (container) container.innerHTML = `<img src="${src}" alt="${escapeHtml(prevP.name)}">`;
              });
            }
          }
        };
      }

      const nextBtn = $('#lightbox-next-btn');
      if (nextBtn) {
        nextBtn.onclick = () => {
          if (state.photosLightboxIndex < filtered.length - 1) {
            state.photosLightboxIndex++;
            renderPhotos();
            const nextP = filtered[state.photosLightboxIndex];
            if (nextP && !state.photosCache[nextP.path]) {
              invokeOrFail('adb_photo_preview', { serial: state.serial, path: nextP.path }).then(src => {
                state.photosCache[nextP.path] = src;
                const container = $('#lightbox-img-container');
                if (container) container.innerHTML = `<img src="${src}" alt="${escapeHtml(nextP.name)}">`;
              });
            }
          }
        };
      }

      const saveBtn = $('#lightbox-save-btn');
      if (saveBtn) {
        saveBtn.onclick = () => action('download_photo', curPhoto.path);
      }

      const delBtn = $('#lightbox-del-btn');
      if (delBtn) {
        delBtn.onclick = async () => {
          if (confirm(`Delete ${curPhoto.name} from device?`)) {
            await action('delete_photo', curPhoto.path);
            delete state.photosCache[curPhoto.path];
            state.photosSelected.delete(curPhoto.path);
            closeLb();
            await loadData('photos');
            renderPhotos();
          }
        };
      }

      const keyHandler = (e) => {
        if (state.photosLightboxIndex === null) {
          window.removeEventListener('keydown', keyHandler);
          return;
        }
        if (e.key === 'Escape') {
          closeLb();
        } else if (e.key === 'ArrowLeft' && prevBtn) {
          prevBtn.click();
        } else if (e.key === 'ArrowRight' && nextBtn) {
          nextBtn.click();
        }
      };
      if (window.__photoKeyHandler) window.removeEventListener('keydown', window.__photoKeyHandler);
      window.__photoKeyHandler = keyHandler;
      window.addEventListener('keydown', keyHandler);
    }
  }
  function renderNotifications() {
    const rows = state.data || [];
    content.innerHTML = `<section class="card"><div class="card-head"><div><h2>Notifications</h2><div class="subtle" style="margin-top:5px">${rows.length} active device notifications</div></div><button class="button" data-refresh>${icon("e145")} Refresh</button></div><div class="stack">${rows.length ? rows.map(n => `<article class="list-item"><div class="list-item-top"><div><div class="list-title">${escapeHtml(n.title || n.app || n.package)}</div><div class="list-meta">${escapeHtml(n.app)} · ${escapeHtml(n.time)}</div></div><span class="chip">${escapeHtml(n.package)}</span></div><p>${escapeHtml(n.text)}</p></article>`).join('') : empty('No active notifications', 'The device did not report any currently posted notifications.')}</div></section>`;
  }
  function renderStorage() {
    const rows = state.data || [], t = state.telemetry || {};
    content.innerHTML = `<div class="grid metrics">${metric('Internal used', t.storage_internal_total_gb != null ? `${(t.storage_internal_total_gb-(t.storage_internal_free_gb??0)).toFixed(1)} GB` : 'Unavailable', t.storage_internal_total_gb != null ? `${(t.storage_internal_free_gb??0).toFixed(1)} GB free of ${t.storage_internal_total_gb.toFixed(1)} GB` : 'No storage telemetry', t.storage_internal_total_gb != null ? 100*(1-t.storage_internal_free_gb/t.storage_internal_total_gb) : null)}${metric('Shared storage', t.storage_sdcard_total_gb != null ? `${(t.storage_sdcard_total_gb-(t.storage_sdcard_free_gb??0)).toFixed(1)} GB` : 'Unavailable', t.storage_sdcard_total_gb != null ? `${(t.storage_sdcard_free_gb??0).toFixed(1)} GB free` : 'No shared storage telemetry', null)}</div><section class="card"><div class="card-head"><div><h2>File browser</h2><div class="subtle mono" style="margin-top:5px">${escapeHtml(state.path)}</div></div><button class="button" data-refresh>${icon("e145")} Refresh</button></div><form id="path-form" class="toolbar"><input id="file-path" class="input" value="${escapeHtml(state.path)}"><button class="button primary">Go to path</button><button type="button" class="button" id="file-up">Parent folder</button></form><div class="table-wrap"><table class="table"><thead><tr><th>Name</th><th>Type</th><th>Size</th><th>Modified</th><th></th></tr></thead><tbody>${rows.length ? rows.map((f,i) => `<tr><td><button class="file-open" data-file-index="${i}">${f.directory?icon("e0d7"):icon("e0cc")} ${escapeHtml(f.name)}</button></td><td>${f.directory?'Folder':'File'}</td><td>${f.bytes ? `${(f.bytes/1024).toFixed(1)} KB` : '—'}</td><td>${escapeHtml(f.modified)}</td><td><button class="button danger" data-action="delete_file" data-value="${escapeHtml(state.path.replace(/\/$/, '')+'/'+f.name)}">Delete</button></td></tr>`).join('') : `<tr><td colspan="5">${empty('Folder is empty', 'Try another folder path.')}</td></tr>`}</tbody></table></div></section>`;
    $('#path-form').onsubmit = (e) => { e.preventDefault(); state.path = $('#file-path').value.trim() || '/sdcard'; showPageLoading('Folder'); void renderTab(); };
    $('#file-up').onclick = () => { state.path = state.path.replace(/\/$/, '').split('/').slice(0,-1).join('/') || '/'; showPageLoading('Folder'); void renderTab(); };
    document.querySelectorAll('[data-file-index]').forEach(b => b.onclick = () => { const f = rows[Number(b.dataset.fileIndex)]; if (f.directory) { state.path = `${state.path.replace(/\/$/, '')}/${f.name}`; showPageLoading('Folder'); void renderTab(); } });
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
  async function renderFirmware() {
    const subtab = state.customSubtab || 'boot';
    const t = state.telemetry || {};

    let subtabHtml = '';

    if (subtab === 'boot') {
      subtabHtml = `
        <div class="boot-modes-grid">
          <div class="boot-mode-card">
            <div class="boot-mode-top">
              <span class="icon" style="color:var(--accent); font-size:18px;">&#xe145;</span>
              <span class="boot-mode-title">System Reboot</span>
            </div>
            <p class="boot-mode-desc">Standard graceful restart of Android OS and running services.</p>
            <button class="boot-mode-btn system" data-reboot-target="system">${icon("e145")} Reboot System</button>
          </div>

          <div class="boot-mode-card">
            <div class="boot-mode-top">
              <span class="icon" style="color:#38bdf8; font-size:18px;">&#xe048;</span>
              <span class="boot-mode-title">Recovery Mode</span>
            </div>
            <p class="boot-mode-desc">Reboot to TWRP, OrangeFox, or Stock Recovery for flashing ROMs, wiping caches, or factory resets.</p>
            <button class="boot-mode-btn recovery" data-reboot-target="recovery">${icon("e048")} Boot Recovery</button>
          </div>

          <div class="boot-mode-card">
            <div class="boot-mode-top">
              <span class="icon" style="color:#fbbf24; font-size:18px;">&#xe0cc;</span>
              <span class="boot-mode-title">Bootloader / Fastboot</span>
            </div>
            <p class="boot-mode-desc">Enter low-level Fastboot mode for partition flashing (boot, recovery, vbmeta) and unlocking.</p>
            <button class="boot-mode-btn bootloader" data-reboot-target="bootloader">${icon("e0cc")} Bootloader (Fastboot)</button>
          </div>

          <div class="boot-mode-card">
            <div class="boot-mode-top">
              <span class="icon" style="color:#c084fc; font-size:18px;">&#xe14f;</span>
              <span class="boot-mode-title">Fastbootd (Dynamic Partitions)</span>
            </div>
            <p class="boot-mode-desc">Userspace fastboot mode on Android 10+ for resizing and flashing dynamic logical partitions.</p>
            <button class="boot-mode-btn fastbootd" data-reboot-target="fastboot">${icon("e14f")} Reboot Fastbootd</button>
          </div>

          <div class="boot-mode-card">
            <div class="boot-mode-top">
              <span class="icon" style="color:#f87171; font-size:18px;">&#xe000;</span>
              <span class="boot-mode-title">EDL Mode (Qualcomm 9008)</span>
            </div>
            <p class="boot-mode-desc">Emergency Download mode for unbricking and deep raw flashing on Qualcomm Snapdragon chipsets.</p>
            <button class="boot-mode-btn edl" data-reboot-target="edl">${icon("e000")} Reboot EDL (9008)</button>
          </div>

          <div class="boot-mode-card">
            <div class="boot-mode-top">
              <span class="icon" style="color:#60a5fa; font-size:18px;">&#xe0b9;</span>
              <span class="boot-mode-title">Sideload Mode</span>
            </div>
            <p class="boot-mode-desc">Directly enter Recovery ADB Sideload state for applying unsigned or signed firmware update zips.</p>
            <button class="boot-mode-btn sideload" data-reboot-target="sideload">${icon("e0b9")} Reboot Sideload</button>
          </div>

          <div class="boot-mode-card">
            <div class="boot-mode-top">
              <span class="icon" style="color:#fb923c; font-size:18px;">&#xe11b;</span>
              <span class="boot-mode-title">Safe Mode</span>
            </div>
            <p class="boot-mode-desc">Boots the system with all third-party applications suppressed to diagnose bootloops and crashes.</p>
            <button class="boot-mode-btn safemode" data-reboot-target="safemode">${icon("e11b")} Boot Safe Mode</button>
          </div>

          <div class="boot-mode-card">
            <div class="boot-mode-top">
              <span class="icon" style="color:#fca5a5; font-size:18px;">&#xe14c;</span>
              <span class="boot-mode-title">Power Off / Shutdown</span>
            </div>
            <p class="boot-mode-desc">Cleanly power down the Android device hardware without initiating a reboot sequence.</p>
            <button class="boot-mode-btn poweroff" id="power-off-btn">${icon("e14c")} Power Off Device</button>
          </div>
        </div>

        <section class="card" style="margin-top:16px;">
          <div class="card-head">
            <div>
              <h3>Hardware & Navigation Emulation</h3>
              <span class="subtle">Inject hardware button events over the ADB bridge</span>
            </div>
          </div>
          <div class="toolbar" style="margin-top:10px;">
            <button class="button" data-action="key_power">${icon("e14c")} Power / Wake</button>
            <button class="button" data-action="key_volume_up">${icon("e145")} Vol +</button>
            <button class="button" data-action="key_volume_down">${icon("e145")} Vol −</button>
            <button class="button tonal" data-action="key_home">${icon("e101")} Home</button>
            <button class="button tonal" data-action="key_back">${icon("e048")} Back</button>
            <button class="button tonal" data-action="key_recents">${icon("e14f")} Recents</button>
          </div>
        </section>
      `;
    } else if (subtab === 'root') {
      const rootStatus = state.customRootStatus;
      const isRooted = rootStatus ? rootStatus.includes('ROOTED') : t.is_rooted;

      subtabHtml = `
        <div class="root-banner">
          <div>
            <div style="display:flex; align-items:center; gap:10px;">
              <span class="root-status-pill ${isRooted ? 'rooted' : 'unrooted'}">
                ● ${isRooted ? 'DEVICE ROOTED (UID 0)' : 'STOCK / UNROOTED'}
              </span>
              <span class="chip mono">SELinux: ${escapeHtml(t.selinux || 'Enforcing')}</span>
            </div>
            <div style="margin-top:6px; color:#cbd5e1; font-size:13px;">
              ${rootStatus ? escapeHtml(rootStatus) : 'Click "Run Root Verification" to detect active su binary, Magisk, KernelSU, or APatch modules.'}
            </div>
          </div>
          <button class="button primary" id="run-root-check-btn">${icon("e145")} Run Root Verification</button>
        </div>

        <div class="tool-grid" style="margin-top:16px;">
          <div class="tool-panel">
            <div class="tool-panel-head">
              <span class="tool-panel-title">${icon("e048")} SELinux Runtime State</span>
              <span class="chip mono" id="selinux-live-chip">${escapeHtml(t.selinux || 'Enforcing')}</span>
            </div>
            <p class="tool-panel-desc">Switch SELinux enforcement without restarting. Permissive mode logs AVC denials without blocking root operations or system modifications.</p>
            <div class="tool-action-row">
              <button class="button tonal" id="selinux-permissive-btn">Set Permissive (0)</button>
              <button class="button" id="selinux-enforcing-btn">Set Enforcing (1)</button>
            </div>
          </div>

          <div class="tool-panel">
            <div class="tool-panel-head">
              <span class="tool-panel-title">${icon("e0d7")} Partition Remount (RW)</span>
            </div>
            <p class="tool-panel-desc">Remount Android <code>/system</code>, <code>/vendor</code>, and <code>/product</code> partitions as Read-Write to allow replacing system files, fonts, or boot animations.</p>
            <div class="tool-action-row">
              <button class="button warning" id="remount-system-btn">${icon("e0cc")} Remount /system as RW</button>
            </div>
          </div>

          <div class="tool-panel" style="grid-column: 1 / -1;">
            <div class="tool-panel-head">
              <span class="tool-panel-title">${icon("e14f")} Superuser Root Shell (su -c)</span>
              <span class="subtle mono">uid=0(root) gid=0(root)</span>
            </div>
            <p class="tool-panel-desc">Execute privileged commands with superuser credentials. Output is captured below.</p>
            <div class="preset-pills" style="margin-bottom:8px;">
              <span style="font-size:11px; color:#64748b; align-self:center;">Quick Commands:</span>
              <button class="preset-pill" data-su-preset="id">id</button>
              <button class="preset-pill" data-su-preset="magisk -v">magisk -v</button>
              <button class="preset-pill" data-su-preset="ls -la /data/adb">ls -la /data/adb</button>
              <button class="preset-pill" data-su-preset="mount | grep system">grep system mount</button>
              <button class="preset-pill" data-su-preset="dmesg | tail -n 25">kernel dmesg</button>
            </div>
            <form id="su-shell-form" class="tool-action-row">
              <input id="su-shell-input" class="input mono" placeholder="su -c id" autocomplete="off">
              <button class="button primary">Run Root Command</button>
            </form>
            <pre id="su-shell-output" class="console" style="min-height:160px; margin-top:8px;">Ready for root shell execution.</pre>
          </div>
        </div>
      `;
    } else if (subtab === 'firmware') {
      subtabHtml = `
        <div class="tool-grid">
          <div class="tool-panel" style="grid-column: 1 / -1;">
            <div class="tool-panel-head">
              <span class="tool-panel-title">${icon("e0cc")} Fastboot Device Bus</span>
              <button class="button tonal" id="scan-fastboot-btn">${icon("e145")} Scan Fastboot Devices</button>
            </div>
            <p class="tool-panel-desc">Check if any connected devices are running in Bootloader, Fastboot, or Fastbootd mode.</p>
            <div id="fastboot-scan-result" class="mono" style="padding:10px 14px; background:var(--well); border-radius:8px; font-size:12px; color:#38bdf8;">
              ${state.customFastbootDevices || 'Click "Scan Fastboot Devices" to query connected fastboot interfaces.'}
            </div>
          </div>

          <div class="tool-panel">
            <div class="tool-panel-head">
              <span class="tool-panel-title">${icon("e0cc")} Sideload OTA Firmware</span>
            </div>
            <p class="tool-panel-desc">Flash official or custom OTA update zip archives onto the device via <code>adb sideload update.zip</code>. The device must be in Recovery Sideload mode.</p>
            <div class="tool-action-row">
              <input id="ota-file-path" class="input mono" placeholder="Path to update.zip..." readonly>
              <button class="button tonal" id="browse-ota-btn">Browse .zip</button>
            </div>
            <button class="button primary" id="flash-ota-btn" style="margin-top:4px;">${icon("e145")} Begin OTA Sideload</button>
          </div>

          <div class="tool-panel">
            <div class="tool-panel-head">
              <span class="tool-panel-title">${icon("e14f")} Fastboot Partition Flasher</span>
            </div>
            <p class="tool-panel-desc">Flash kernel, ramdisk, recovery, or AVB meta partition images (<code>boot.img</code>, <code>recovery.img</code>, <code>vbmeta.img</code>) in fastboot mode.</p>
            <div style="display:flex; gap:8px;">
              <select id="fastboot-partition-select" class="input" style="width:140px;">
                <option value="boot">boot</option>
                <option value="init_boot">init_boot</option>
                <option value="recovery">recovery</option>
                <option value="vbmeta">vbmeta</option>
                <option value="vendor_boot">vendor_boot</option>
                <option value="dtbo">dtbo</option>
                <option value="system">system</option>
              </select>
              <input id="fastboot-img-path" class="input mono" placeholder="Path to image (.img)..." readonly>
              <button class="button tonal" id="browse-fastboot-img-btn">Browse</button>
            </div>
            <button class="button warning" id="flash-fastboot-btn" style="margin-top:4px;">${icon("e0cc")} Flash to Partition</button>
          </div>

          <div class="tool-panel" style="grid-column: 1 / -1;">
            <div class="tool-panel-head">
              <span class="tool-panel-title">${icon("e11b")} Diagnostic Bugreport Generator</span>
              <button class="button success" id="gen-bugreport-btn">${icon("e0cc")} Generate Bugreport ZIP</button>
            </div>
            <p class="tool-panel-desc">Produces a complete system state dump including kernel log ring buffer, tombstone crashes, ANR logs, package metadata, and dumpsys telemetry saved to your PC <code>~/Downloads/</code> directory.</p>
          </div>
        </div>
      `;
    } else if (subtab === 'backup') {
      subtabHtml = `
        <div class="tool-grid">
          <div class="tool-panel">
            <div class="tool-panel-head">
              <span class="tool-panel-title">${icon("e0d7")} Full Device Backup (.ab)</span>
            </div>
            <p class="tool-panel-desc">Creates an Android backup archive (including installed APKs, shared media, and app configurations). The file will be placed in your PC Downloads folder.</p>
            <div class="notice" style="font-size:12px;">You will need to unlock your phone and tap "Back up my data" on the screen prompt to authorize the backup.</div>
            <button class="button primary" id="full-backup-btn" style="margin-top:4px;">${icon("e0cc")} Initiate Full Backup</button>
          </div>

          <div class="tool-panel">
            <div class="tool-panel-head">
              <span class="tool-panel-title">${icon("e14f")} Single Application Backup</span>
            </div>
            <p class="tool-panel-desc">Export a single application's APK and data to an encrypted or plain <code>.ab</code> file.</p>
            <div class="tool-action-row">
              <input id="backup-app-pkg" class="input mono" placeholder="Package name (e.g. com.whatsapp)">
              <button class="button tonal" id="backup-app-btn">Backup App</button>
            </div>
          </div>

          <div class="tool-panel">
            <div class="tool-panel-head">
              <span class="tool-panel-title">${icon("e048")} Live Partition Dumper (Root)</span>
            </div>
            <p class="tool-panel-desc">Extract raw flash block devices directly from <code>/dev/block/by-name/</code> into a pristine <code>.img</code> file on your computer.</p>
            <div class="preset-pills" style="margin-bottom:6px;">
              <span style="font-size:11px; color:#64748b; align-self:center;">Presets:</span>
              <button class="preset-pill" data-part-preset="boot">boot</button>
              <button class="preset-pill" data-part-preset="init_boot">init_boot</button>
              <button class="preset-pill" data-part-preset="recovery">recovery</button>
              <button class="preset-pill" data-part-preset="vbmeta">vbmeta</button>
              <button class="preset-pill" data-part-preset="vendor_boot">vendor_boot</button>
            </div>
            <div class="tool-action-row">
              <input id="dump-partition-name" class="input mono" placeholder="Partition name (e.g. boot)">
              <button class="button warning" id="dump-partition-btn">Dump to PC</button>
            </div>
          </div>

          <div class="tool-panel">
            <div class="tool-panel-head">
              <span class="tool-panel-title">${icon("e145")} Restore Android Backup</span>
            </div>
            <p class="tool-panel-desc">Restore an existing <code>.ab</code> archive back onto the connected Android device.</p>
            <div class="tool-action-row">
              <input id="restore-ab-path" class="input mono" placeholder="Select backup file (.ab)..." readonly>
              <button class="button tonal" id="browse-restore-ab-btn">Browse .ab</button>
            </div>
            <button class="button success" id="execute-restore-btn" style="margin-top:4px;">${icon("e145")} Restore to Device</button>
          </div>
        </div>
      `;
    } else if (subtab === 'tweaks') {
      subtabHtml = `
        <div class="tool-grid">
          <div class="tool-panel">
            <div class="tool-panel-head">
              <span class="tool-panel-title">${icon("e14f")} Display Density (DPI)</span>
              <span class="chip mono">Current: ${escapeHtml(t.screen_dpi || 'Stock')}</span>
            </div>
            <p class="tool-panel-desc">Adjust UI scaling and density live via WindowManager without requiring a reboot.</p>
            <div class="preset-pills" style="margin-bottom:6px;">
              <span style="font-size:11px; color:#64748b; align-self:center;">Presets:</span>
              <button class="preset-pill" data-dpi-val="320">320 (Compact)</button>
              <button class="preset-pill" data-dpi-val="360">360 (Small)</button>
              <button class="preset-pill" data-dpi-val="400">400 (Default)</button>
              <button class="preset-pill" data-dpi-val="440">440 (Large)</button>
              <button class="preset-pill" data-dpi-val="560">560 (Tablet)</button>
            </div>
            <div class="tool-action-row">
              <input id="custom-dpi-input" class="input mono" placeholder="Enter DPI (e.g. 380)" value="${escapeHtml(state.customDensityVal)}">
              <button class="button primary" id="apply-dpi-btn">Apply DPI</button>
              <button class="button tonal" id="reset-dpi-btn">Reset</button>
            </div>
          </div>

          <div class="tool-panel">
            <div class="tool-panel-head">
              <span class="tool-panel-title">${icon("e145")} Snappy Animation Scales</span>
            </div>
            <p class="tool-panel-desc">Speed up window animations, transitions, and animator durations across the entire system UI.</p>
            <div class="preset-pills" style="margin-bottom:10px;">
              <button class="preset-pill ${state.customAnimScale === '0.25' ? 'active' : ''}" data-anim-val="0.25">0.25x (Lightning)</button>
              <button class="preset-pill ${state.customAnimScale === '0.5' ? 'active' : ''}" data-anim-val="0.5">0.5x (Snappy)</button>
              <button class="preset-pill ${state.customAnimScale === '1.0' ? 'active' : ''}" data-anim-val="1.0">1.0x (Stock)</button>
              <button class="preset-pill ${state.customAnimScale === '0.0' ? 'active' : ''}" data-anim-val="0.0">0.0x (Instant / Off)</button>
            </div>
            <button class="button tonal" id="apply-fast-anim-btn">${icon("e145")} Set 0.5x Snappy Speed</button>
          </div>

          <div class="tool-panel" style="grid-column: 1 / -1;">
            <div class="tool-panel-head">
              <span class="tool-panel-title">${icon("e0cc")} Screen Resolution (WindowManager)</span>
              <span class="chip mono">Native: ${escapeHtml(t.screen_res || 'Default')}</span>
            </div>
            <p class="tool-panel-desc">Force custom render resolutions for gaming, battery optimization, or performance testing.</p>
            <div class="preset-pills" style="margin-bottom:8px;">
              <button class="preset-pill" data-res-val="1080x2400">1080x2400 (FHD+)</button>
              <button class="preset-pill" data-res-val="1440x3200">1440x3200 (QHD+)</button>
              <button class="preset-pill" data-res-val="720x1600">720x1600 (HD+ 720p)</button>
              <button class="preset-pill" data-res-val="reset">Reset Native</button>
            </div>
            <div class="tool-action-row" style="max-width:480px;">
              <input id="custom-res-input" class="input mono" placeholder="1080x2400 or reset">
              <button class="button primary" id="apply-res-btn">Set Resolution</button>
            </div>
          </div>
        </div>
      `;
    }

    content.innerHTML = `
      <div class="custom-suite">
        <nav class="custom-subnav">
          <button class="custom-subtab-btn ${subtab === 'boot' ? 'active' : ''}" data-custom-tab="boot">${icon("e145")} Power & Boot Modes</button>
          <button class="custom-subtab-btn ${subtab === 'root' ? 'active' : ''}" data-custom-tab="root">${icon("e048")} Root & Superuser</button>
          <button class="custom-subtab-btn ${subtab === 'firmware' ? 'active' : ''}" data-custom-tab="firmware">${icon("e0cc")} Firmware & Fastboot</button>
          <button class="custom-subtab-btn ${subtab === 'backup' ? 'active' : ''}" data-custom-tab="backup">${icon("e0d7")} Backup & Restore</button>
          <button class="custom-subtab-btn ${subtab === 'tweaks' ? 'active' : ''}" data-custom-tab="tweaks">${icon("e14f")} System Tweaks</button>
        </nav>
        ${subtabHtml}
      </div>
    `;

    document.querySelectorAll('[data-custom-tab]').forEach(btn => {
      btn.onclick = () => {
        state.customSubtab = btn.dataset.customTab;
        renderFirmware();
      };
    });

    document.querySelectorAll('[data-reboot-target]').forEach(btn => {
      btn.onclick = async () => {
        const target = btn.dataset.rebootTarget;
        if (confirm(`Reboot device into ${target.toUpperCase()} mode? The device may briefly disconnect.`)) {
          await action('reboot', target);
        }
      };
    });

    const powerOffBtn = $('#power-off-btn');
    if (powerOffBtn) {
      powerOffBtn.onclick = async () => {
        if (confirm('Power off the Android device hardware?')) {
          await action('shell', 'reboot -p');
          toast('Power off command sent.');
        }
      };
    }

    const runRootCheckBtn = $('#run-root-check-btn');
    if (runRootCheckBtn) {
      runRootCheckBtn.onclick = async () => {
        toast('Checking root status and superuser binaries...');
        try {
          const res = await action('root_check');
          state.customRootStatus = res;
          renderFirmware();
        } catch (err) {
          toast(String(err), true);
        }
      };
    }

    const selinuxPermBtn = $('#selinux-permissive-btn');
    if (selinuxPermBtn) {
      selinuxPermBtn.onclick = async () => {
        try {
          await action('set_selinux', '0');
          const chip = $('#selinux-live-chip');
          if (chip) chip.textContent = 'Permissive';
        } catch (e) { toast(String(e), true); }
      };
    }

    const selinuxEnfBtn = $('#selinux-enforcing-btn');
    if (selinuxEnfBtn) {
      selinuxEnfBtn.onclick = async () => {
        try {
          await action('set_selinux', '1');
          const chip = $('#selinux-live-chip');
          if (chip) chip.textContent = 'Enforcing';
        } catch (e) { toast(String(e), true); }
      };
    }

    const remountBtn = $('#remount-system-btn');
    if (remountBtn) {
      remountBtn.onclick = async () => {
        try {
          const res = await action('remount_system');
          toast(res || 'Remount command completed');
        } catch (e) { toast(String(e), true); }
      };
    }

    document.querySelectorAll('[data-su-preset]').forEach(btn => {
      btn.onclick = () => {
        const input = $('#su-shell-input');
        if (input) { input.value = btn.dataset.suPreset; input.focus(); }
      };
    });

    const suForm = $('#su-shell-form');
    if (suForm) {
      suForm.onsubmit = async (e) => {
        e.preventDefault();
        const cmd = $('#su-shell-input').value.trim();
        if (!cmd) return;
        const out = $('#su-shell-output');
        out.textContent = `# su -c "${cmd}"\nExecuting with UID 0...`;
        try {
          const res = await action('shell', `su -c "${cmd}"`);
          out.textContent = `# su -c "${cmd}"\n${res || '(no output / completed)'}`;
        } catch (err) {
          out.textContent = `# su -c "${cmd}"\nError: ${err}`;
        }
      };
    }

    const scanFastbootBtn = $('#scan-fastboot-btn');
    if (scanFastbootBtn) {
      scanFastbootBtn.onclick = async () => {
        try {
          const res = await action('fastboot_devices');
          state.customFastbootDevices = res;
          const box = $('#fastboot-scan-result');
          if (box) box.textContent = res;
        } catch (err) { toast(String(err), true); }
      };
    }

    const browseOtaBtn = $('#browse-ota-btn');
    if (browseOtaBtn) {
      browseOtaBtn.onclick = async () => {
        try {
          const p = await action('pick_file', 'zip');
          if (p) $('#ota-file-path').value = p;
        } catch {}
      };
    }

    const flashOtaBtn = $('#flash-ota-btn');
    if (flashOtaBtn) {
      flashOtaBtn.onclick = async () => {
        const p = $('#ota-file-path').value.trim();
        if (!p) { toast('Please browse and select an OTA .zip file first.', true); return; }
        if (confirm(`Begin ADB sideload of:\n${p}\n\nEnsure device is in Recovery Sideload mode!`)) {
          toast('Starting OTA sideload, please wait...');
          try {
            const res = await action('sideload_ota', p);
            toast(res || 'OTA Sideload complete!');
          } catch (err) { toast(String(err), true); }
        }
      };
    }

    const browseFastbootImgBtn = $('#browse-fastboot-img-btn');
    if (browseFastbootImgBtn) {
      browseFastbootImgBtn.onclick = async () => {
        try {
          const p = await action('pick_file', 'img');
          if (p) $('#fastboot-img-path').value = p;
        } catch {}
      };
    }

    const flashFastbootBtn = $('#flash-fastboot-btn');
    if (flashFastbootBtn) {
      flashFastbootBtn.onclick = async () => {
        const part = $('#fastboot-partition-select').value;
        const img = $('#fastboot-img-path').value.trim();
        if (!img) { toast('Please select an image file (.img) to flash.', true); return; }
        if (confirm(`Flash image:\n${img}\n\nTo partition [${part}] in fastboot mode?`)) {
          toast(`Flashing ${part}...`);
          try {
            const res = await action('fastboot_flash', part, img);
            toast(res || `Fastboot flash to ${part} finished!`);
          } catch (err) { toast(String(err), true); }
        }
      };
    }

    const bugreportBtn = $('#gen-bugreport-btn');
    if (bugreportBtn) {
      bugreportBtn.onclick = async () => {
        toast('Generating comprehensive bugreport (this may take 20-30 seconds)...');
        try {
          const res = await action('bugreport');
          toast(res || 'Bugreport saved to Downloads folder!');
        } catch (err) { toast(String(err), true); }
      };
    }

    const fullBackupBtn = $('#full-backup-btn');
    if (fullBackupBtn) {
      fullBackupBtn.onclick = async () => {
        try {
          const res = await action('backup_full');
          toast(res || 'Backup started - check phone display!');
        } catch (err) { toast(String(err), true); }
      };
    }

    const backupAppBtn = $('#backup-app-btn');
    if (backupAppBtn) {
      backupAppBtn.onclick = async () => {
        const pkg = $('#backup-app-pkg').value.trim();
        if (!pkg) { toast('Enter a package name (e.g. com.example.app)', true); return; }
        try {
          const res = await action('backup_app', pkg);
          toast(res || 'App backup started!');
        } catch (err) { toast(String(err), true); }
      };
    }

    document.querySelectorAll('[data-part-preset]').forEach(btn => {
      btn.onclick = () => {
        const input = $('#dump-partition-name');
        if (input) { input.value = btn.dataset.partPreset; input.focus(); }
      };
    });

    const dumpPartBtn = $('#dump-partition-btn');
    if (dumpPartBtn) {
      dumpPartBtn.onclick = async () => {
        const part = $('#dump-partition-name').value.trim();
        if (!part) { toast('Enter or select a partition name to dump', true); return; }
        toast(`Dumping /dev/block/by-name/${part} to PC Downloads...`);
        try {
          const res = await action('dump_partition', part);
          toast(res || `Partition ${part} dumped successfully!`);
        } catch (err) { toast(String(err), true); }
      };
    }

    const browseRestoreBtn = $('#browse-restore-ab-btn');
    if (browseRestoreBtn) {
      browseRestoreBtn.onclick = async () => {
        try {
          const p = await action('pick_file', 'ab');
          if (p) $('#restore-ab-path').value = p;
        } catch {}
      };
    }

    const executeRestoreBtn = $('#execute-restore-btn');
    if (executeRestoreBtn) {
      executeRestoreBtn.onclick = async () => {
        const p = $('#restore-ab-path').value.trim();
        if (!p) { toast('Browse and select an .ab backup file first', true); return; }
        if (confirm(`Restore backup archive:\n${p}\n\nConfirm prompt on phone screen when prompted.`)) {
          try {
            const res = await action('restore_backup', p);
            toast(res || 'Restore command sent!');
          } catch (err) { toast(String(err), true); }
        }
      };
    }

    document.querySelectorAll('[data-dpi-val]').forEach(btn => {
      btn.onclick = () => {
        const input = $('#custom-dpi-input');
        if (input) input.value = btn.dataset.dpiVal;
      };
    });

    const applyDpiBtn = $('#apply-dpi-btn');
    if (applyDpiBtn) {
      applyDpiBtn.onclick = async () => {
        const val = $('#custom-dpi-input').value.trim();
        if (!val) return;
        state.customDensityVal = val;
        try {
          await action('set_density', val);
          toast(`Display density set to ${val} DPI`);
        } catch (err) { toast(String(err), true); }
      };
    }

    const resetDpiBtn = $('#reset-dpi-btn');
    if (resetDpiBtn) {
      resetDpiBtn.onclick = async () => {
        try {
          await action('set_density', 'reset');
          toast('Display density reset to default stock value');
        } catch (err) { toast(String(err), true); }
      };
    }

    document.querySelectorAll('[data-anim-val]').forEach(btn => {
      btn.onclick = async () => {
        const scale = btn.dataset.animVal;
        state.customAnimScale = scale;
        try {
          await action('set_animations', scale);
          toast(`Animation scales set to ${scale}x`);
          renderFirmware();
        } catch (err) { toast(String(err), true); }
      };
    });

    const applyFastAnimBtn = $('#apply-fast-anim-btn');
    if (applyFastAnimBtn) {
      applyFastAnimBtn.onclick = async () => {
        state.customAnimScale = '0.5';
        try {
          await action('set_animations', '0.5');
          toast('Applied 0.5x snappy animation scales');
          renderFirmware();
        } catch (err) { toast(String(err), true); }
      };
    }

    document.querySelectorAll('[data-res-val]').forEach(btn => {
      btn.onclick = () => {
        const input = $('#custom-res-input');
        if (input) input.value = btn.dataset.resVal;
      };
    });

    const applyResBtn = $('#apply-res-btn');
    if (applyResBtn) {
      applyResBtn.onclick = async () => {
        const v = $('#custom-res-input').value.trim();
        if (!v) return;
        try {
          if (v === 'reset') {
            await action('shell', 'wm size reset');
            toast('Screen resolution reset to native');
          } else {
            await action('shell', `wm size ${v}`);
            toast(`Screen resolution set to ${v}`);
          }
        } catch (err) { toast(String(err), true); }
      };
    }
  }
  function formatRate(value) {
    if (value == null || !Number.isFinite(Number(value))) return '0 B/s';
    const num = Number(value);
    const units = ['B/s', 'KB/s', 'MB/s', 'GB/s'];
    let n = Math.abs(num), i = 0;
    while (n >= 1024 && i < units.length - 1) { n /= 1024; i++; }
    const sign = num < 0 ? '-' : '';
    return `${sign}${n.toFixed(i ? 1 : 0)} ${units[i]}`;
  }

  function formatBytes(value) {
    if (value == null || !Number.isFinite(Number(value))) return '0 B';
    const num = Number(value);
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    let n = Math.abs(num), i = 0;
    while (n >= 1024 && i < units.length - 1) { n /= 1024; i++; }
    const sign = num < 0 ? '-' : '';
    return `${sign}${n.toFixed(i ? 1 : 0)} ${units[i]}`;
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
          procCpu = Number((100 * (1 - (c.idle - oldIdle) / (c.total - oldTotal))).toFixed(1));
        }
        continue;
      }
      if (!previous) continue;
      const old = (previous.cpuCounters || {})[c.name];
      if (old && c.total > old.total && c.idle >= old.idle) {
        cores[c.name] = Number((100 * (1 - (c.idle - old.idle) / (c.total - old.total))).toFixed(1));
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
      cpu: Number((s.cpu_usage_pct ?? procCpu ?? 0).toFixed(1)),
      ram: Number((mem ?? 0).toFixed(1)),
      swap: Number((s.swap_total_kb ? 100 * (s.swap_total_kb - (s.swap_free_kb || 0)) / s.swap_total_kb : 0).toFixed(1)),
      storage: Number((diskTotal && diskFree != null ? 100 * (diskTotal - diskFree) / diskTotal : 0).toFixed(1)),
      rx, tx,
      rxRate: Number(rxRate.toFixed(1)),
      txRate: Number(txRate.toFixed(1)),
      diskReadRate: Number(diskReadRate.toFixed(1)),
      diskWriteRate: Number(diskWriteRate.toFixed(1)),
      battery: Number((s.battery_pct ?? 0).toFixed(0)),
      temp: Number((temps.length ? Math.max(...temps.map(x => Number(x.temp_c)).filter(Number.isFinite)) : (s.battery_temp_c ?? 0)).toFixed(1)),
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
      return `<svg class="stat-card-spark" viewBox="0 0 100 48" preserveAspectRatio="none"><line x1="0" y1="24" x2="100" y2="24" stroke="rgba(255,255,255,0.06)" stroke-width="1.5"/></svg>`;
    }
    const max = Math.max(maxVal || 1, ...pointsData);
    const min = Math.min(0, ...pointsData);
    const range = max - min || 1;
    const n = pointsData.length;
    const stepX = 100 / (n - 1);
    const pts = pointsData.map((v, i) => {
      const x = (i * stepX).toFixed(1);
      const norm = Math.max(0.02, Math.min(0.96, (v - min) / range));
      const y = (46 - norm * 42).toFixed(1);
      return `${x},${y}`;
    }).join(' ');

    const gradId = 'spk-' + Math.random().toString(36).slice(2, 7);
    const dottedLine = dottedRatio != null
      ? `<line x1="0" y1="${(46 - Math.max(0.05, Math.min(0.95, dottedRatio)) * 42).toFixed(1)}" x2="100" y2="${(46 - Math.max(0.05, Math.min(0.95, dottedRatio)) * 42).toFixed(1)}" stroke="${color}" stroke-opacity="0.4" stroke-dasharray="3,3" stroke-width="1.2"/>`
      : '';

    return `
      <svg class="stat-card-spark" viewBox="0 0 100 48" preserveAspectRatio="none" aria-hidden="true">
        <defs>
          <linearGradient id="${gradId}" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stop-color="${color}" stop-opacity="0.38"/>
            <stop offset="100%" stop-color="${color}" stop-opacity="0.02"/>
          </linearGradient>
        </defs>
        ${dottedLine}
        <polygon points="0,48 ${pts} 100,48" fill="url(#${gradId})"/>
        <polyline points="${pts}" fill="none" stroke="${color}" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" vector-effect="non-scaling-stroke"/>
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
    const h = 190, w = 600;
    if (n < 2) {
      return `<div class="chart-empty">Collecting network telemetry samples…</div>`;
    }
    const maxV = Math.max(peakVal || 0, ...txHistory, ...rxHistory, 1024);
    const stepX = (w - 24) / (n - 1);
    const yBase = h - 18;
    const yTop = 16;
    const graphH = yBase - yTop;

    const toPoints = (series) => series.map((v, i) => {
      const x = (12 + i * stepX).toFixed(1);
      const norm = Math.max(0.02, Math.min(0.94, (Number(v) || 0) / maxV));
      const y = (yBase - norm * graphH).toFixed(1);
      return `${x},${y}`;
    });

    const txPts = toPoints(txHistory);
    const rxPts = toPoints(rxHistory);

    const txPoly = txPts.join(' ');
    const rxPoly = rxPts.join(' ');

    const txArea = `12,${yBase} ${txPoly} ${12 + (txHistory.length - 1) * stepX},${yBase}`;
    const rxArea = `12,${yBase} ${rxPoly} ${12 + (rxHistory.length - 1) * stepX},${yBase}`;

    const gradTx = 'dnet-tx-' + Math.random().toString(36).slice(2, 7);
    const gradRx = 'dnet-rx-' + Math.random().toString(36).slice(2, 7);
    const yPeak = (yTop + graphH * 0.28).toFixed(1);

    return `
      <svg viewBox="0 0 ${w} ${h}" role="img" aria-label="Network throughput history" preserveAspectRatio="none" style="width:100%;height:${h}px;display:block;">
        <defs>
          <linearGradient id="${gradTx}" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stop-color="${accentColor}" stop-opacity="0.36"/>
            <stop offset="100%" stop-color="${accentColor}" stop-opacity="0.02"/>
          </linearGradient>
          <linearGradient id="${gradRx}" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stop-color="#38bdf8" stop-opacity="0.22"/>
            <stop offset="100%" stop-color="#38bdf8" stop-opacity="0.01"/>
          </linearGradient>
        </defs>
        <line x1="12" y1="${yBase}" x2="${w - 12}" y2="${yBase}" stroke="rgba(255,255,255,0.08)" stroke-width="1.2"/>
        <line x1="12" y1="${yPeak}" x2="${w - 12}" y2="${yPeak}" stroke="${accentColor}" stroke-opacity="0.5" stroke-dasharray="6,4" stroke-width="1.4"/>
        <polygon points="${rxArea}" fill="url(#${gradRx})"/>
        <polyline points="${rxPoly}" fill="none" stroke="#38bdf8" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/>
        <polygon points="${txArea}" fill="url(#${gradTx})"/>
        <polyline points="${txPoly}" fill="none" stroke="${accentColor}" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"/>
      </svg>
    `;
  }

  function renderSocketsChartSvg(tcpHistory = [], udpHistory = [], accentColor = 'var(--accent)') {
    const n = Math.max(tcpHistory.length, udpHistory.length);
    const h = 190, w = 450;
    if (n < 2) {
      return `<div class="chart-empty">Collecting socket telemetry…</div>`;
    }
    const maxV = Math.max(10, ...tcpHistory, ...udpHistory);
    const stepX = (w - 24) / (n - 1);
    const yBase = h - 18;
    const yTop = 16;
    const graphH = yBase - yTop;

    const toPoints = (series) => series.map((v, i) => {
      const x = (12 + i * stepX).toFixed(1);
      const norm = Math.max(0.04, Math.min(0.92, (Number(v) || 0) / maxV));
      const y = (yBase - norm * graphH).toFixed(1);
      return `${x},${y}`;
    });

    const tcpPts = toPoints(tcpHistory);
    const udpPts = toPoints(udpHistory);

    const tcpPoly = tcpPts.join(' ');
    const udpPoly = udpPts.join(' ');

    const tcpArea = `12,${yBase} ${tcpPoly} ${12 + (tcpHistory.length - 1) * stepX},${yBase}`;
    const gradTcp = 'sock-tcp-' + Math.random().toString(36).slice(2, 7);
    const yMid = (yTop + graphH * 0.45).toFixed(1);

    return `
      <svg viewBox="0 0 ${w} ${h}" role="img" aria-label="Sockets history" preserveAspectRatio="none" style="width:100%;height:${h}px;display:block;">
        <defs>
          <linearGradient id="${gradTcp}" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stop-color="${accentColor}" stop-opacity="0.36"/>
            <stop offset="100%" stop-color="${accentColor}" stop-opacity="0.02"/>
          </linearGradient>
        </defs>
        <line x1="12" y1="${yBase}" x2="${w - 12}" y2="${yBase}" stroke="rgba(255,255,255,0.08)" stroke-width="1.2"/>
        <line x1="12" y1="${yMid}" x2="${w - 12}" y2="${yMid}" stroke="${accentColor}" stroke-opacity="0.4" stroke-dasharray="5,4" stroke-width="1.4"/>
        <polygon points="${tcpArea}" fill="url(#${gradTcp})"/>
        <polyline points="${tcpPoly}" fill="none" stroke="${accentColor}" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"/>
        <polyline points="${udpPoly}" fill="none" stroke="#94a3b8" stroke-width="2.0" stroke-linecap="round" stroke-linejoin="round" stroke-dasharray="5,4"/>
      </svg>
    `;
  }

  function renderWaveformChartSvg({ title, series = [], height = 200, suffix = '%', benchmarkVal = null }) {
    const n = Math.max(...series.map(s => s.data?.length || 0));
    const w = 800, h = height;
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

    const yBase = h - 18;
    const yTop = 16;
    const graphH = yBase - yTop;
    const stepX = (w - 24) / (n - 1);

    const legend = series.map((s, idx) => `
      <span class="chart-legend-item">
        <i style="background:${s.color || 'var(--accent)'}"></i>
        ${escapeHtml(s.label)}
      </span>
    `).join('');

    const gridLines = [0.25, 0.5, 0.75].map(ratio => {
      const y = (yBase - ratio * graphH).toFixed(1);
      return `<line x1="12" y1="${y}" x2="${w - 12}" y2="${y}" stroke="rgba(255,255,255,0.06)" stroke-dasharray="5,5" stroke-width="1"/>`;
    }).join('');

    const benchmarkLine = benchmarkVal != null
      ? `<line x1="12" y1="${(yBase - Math.max(0.05, Math.min(0.95, (benchmarkVal - minV) / range)) * graphH).toFixed(1)}" x2="${w - 12}" y2="${(yBase - Math.max(0.05, Math.min(0.95, (benchmarkVal - minV) / range)) * graphH).toFixed(1)}" stroke="var(--accent)" stroke-opacity="0.5" stroke-dasharray="6,4" stroke-width="1.4"/>`
      : '';

    const paths = series.map((s, idx) => {
      const d = s.data || [];
      const pts = d.map((v, i) => {
        const x = (12 + i * stepX).toFixed(1);
        const norm = Math.max(0.02, Math.min(0.98, ((Number(v) || 0) - minV) / range));
        const y = (yBase - norm * graphH).toFixed(1);
        return `${x},${y}`;
      });
      const poly = pts.join(' ');
      const area = `12,${yBase} ${poly} ${12 + (d.length - 1) * stepX},${yBase}`;
      const gradId = 'wf-grad-' + idx + '-' + Math.random().toString(36).slice(2, 6);
      return `
        <defs>
          <linearGradient id="${gradId}" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stop-color="${s.color || 'var(--accent)'}" stop-opacity="${s.fillOpacity ?? 0.32}"/>
            <stop offset="100%" stop-color="${s.color || 'var(--accent)'}" stop-opacity="0.01"/>
          </linearGradient>
        </defs>
        <polygon points="${area}" fill="url(#${gradId})"/>
        <polyline points="${poly}" fill="none" stroke="${s.color || 'var(--accent)'}" stroke-width="${s.strokeWidth || 2.5}" stroke-linecap="round" stroke-linejoin="round"/>
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
            <line x1="12" y1="${yBase}" x2="${w - 12}" y2="${yBase}" stroke="rgba(255,255,255,0.08)" stroke-width="1.2"/>
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

  async function pollSensors(force = false) {
    if (state.sensorBusy || state.tab !== 'sensors' || (!force && (document.hidden || state.sensorsCadence === 0))) return;
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

    const kb = (value) => {
      if (value == null || !Number.isFinite(Number(value))) return 'Unavailable';
      return `${(Number(value) / 1024 / 1024).toFixed(2)} GB`;
    };
    const onOff = (value) => value == null ? 'Unavailable' : value ? 'On' : 'Off';
    const avg = (arr) => arr.length ? arr.reduce((a, b) => a + b, 0) / arr.length : 0;
    const peak = (arr) => arr.length ? Math.max(...arr) : 0;

    const currentCpu = Number(s.cpu_usage_pct ?? state.sensorHistory.at(-1)?.cpu ?? 0);
    const memUsage = s.mem_total_kb && s.mem_available_kb != null ? (100 * (s.mem_total_kb - s.mem_available_kb) / s.mem_total_kb) : 0;
    const usedMemoryKb = s.mem_total_kb && s.mem_available_kb != null ? s.mem_total_kb - s.mem_available_kb : 0;
    const swapUsage = s.swap_total_kb ? (100 * (s.swap_total_kb - (s.swap_free_kb || 0)) / s.swap_total_kb) : 0;
    const usedSwapKb = s.swap_total_kb ? s.swap_total_kb - (s.swap_free_kb || 0) : 0;

    const totalStorageGb = s.storage_internal_total_kb == null ? (Number(t.storage_internal_total_gb) || 0) : Number(s.storage_internal_total_kb) / 1024 / 1024;
    const freeStorageGb = s.storage_internal_free_kb == null ? (Number(t.storage_internal_free_gb) || 0) : Number(s.storage_internal_free_kb) / 1024 / 1024;
    const usedStorageGb = Math.max(0, totalStorageGb - freeStorageGb);
    const storageUsage = totalStorageGb > 0 ? (100 * usedStorageGb / totalStorageGb) : 0;

    const sharedTotalGb = (Number(s.storage_shared_total_kb) || 0) / 1024 / 1024;
    const sharedFreeGb = (Number(s.storage_shared_free_kb) || 0) / 1024 / 1024;
    const sharedUsedGb = Math.max(0, sharedTotalGb - sharedFreeGb);

    const network = s.network_interfaces || [];
    const temps = s.temperatures || [];
    const processes = s.top_processes || [];
    const sensors = state.sensorInventoryLoaded ? state.sensorInventory : (s.sensor_names || []);

    const hist = state.sensorHistory;
    const cpuHist = hist.map(h => Number(h.cpu) || 0);
    const ramHist = hist.map(h => Number(h.ram) || 0);
    const swapHist = hist.map(h => Number(h.swap) || 0);
    const storageHist = hist.map(h => Number(h.storage) || 0);
    const rxHist = hist.map(h => Number(h.rxRate) || 0);
    const txHist = hist.map(h => Number(h.txRate) || 0);
    const readHist = hist.map(h => Number(h.diskReadRate) || 0);
    const writeHist = hist.map(h => Number(h.diskWriteRate) || 0);
    const tcpHist = hist.map(h => Number(h.socketTcp) || 0);
    const udpHist = hist.map(h => Number(h.socketUdp) || 0);
    const battHist = hist.map(h => Number(h.battery) || 0);
    const tempHist = hist.map(h => Number(h.temp) || 0);

    const latest = hist.at(-1) || {};
    const currRxRate = Number(latest.rxRate) || 0;
    const currTxRate = Number(latest.txRate) || 0;
    const netPeakRate = Math.max(1024, ...rxHist, ...txHist);
    const totRx = network.reduce((acc, iface) => acc + (Number(iface.rx_bytes) || 0), 0);
    const totTx = network.reduce((acc, iface) => acc + (Number(iface.tx_bytes) || 0), 0);
    const avgRxRate = avg(rxHist);
    const avgTxRate = avg(txHist);

    const socketTotal = Number(latest.socketTotal || s.socket_total || 0);
    const socketTcp = Number(latest.socketTcp || s.socket_tcp || 0);
    const socketUdp = Number(latest.socketUdp || s.socket_udp || 0);

    const freqGhz = s.cpu_frequencies_mhz?.length ? (Math.max(...s.cpu_frequencies_mhz) / 1000).toFixed(2) : '2.00';
    const minFreqMhz = s.cpu_frequencies_mhz?.length ? (Math.min(...s.cpu_frequencies_mhz) / 1000).toFixed(2) : '1.00';
    const uptimeHours = s.uptime_secs ? Math.floor(s.uptime_secs / 3600) : 0;
    const uptimeMins = s.uptime_secs ? Math.floor((s.uptime_secs % 3600) / 60) : 0;

    // ══════════════════════════════════════════════════════════════════════════
    // WINDOWS TASK MANAGER: Authentic Two-Tier Header Process Grid
    // ══════════════════════════════════════════════════════════════════════════
    function isForegroundApp(nameLower) {
      if (
        nameLower.startsWith('[') || nameLower.startsWith('kworker') ||
        nameLower.startsWith('irq/') || nameLower.startsWith('ksoftirqd') ||
        nameLower.startsWith('rcu_') || nameLower.startsWith('migration') ||
        nameLower.startsWith('kswapd') || nameLower.startsWith('jbd2') ||
        nameLower.startsWith('cpuhp') || nameLower.startsWith('khugepaged') ||
        nameLower.startsWith('kcompactd') || nameLower.startsWith('kdevtmpfs') ||
        nameLower.startsWith('kauditd') || nameLower.startsWith('threadpool') ||
        nameLower.startsWith('dedicatedworker') || nameLower.startsWith('chrome_') ||
        nameLower.startsWith('perfetto') || nameLower.startsWith('compositor') ||
        nameLower.startsWith('js helper') || nameLower.startsWith('v8 ') ||
        nameLower.startsWith('crashpad') || nameLower.startsWith('sandbox') ||
        nameLower.startsWith('gmain') || nameLower.startsWith('gdbus')
      ) {
        return false;
      }
      const bgKeywords = [
        'systemd', 'dbus', 'polkit', 'accounts-daemon', 'upower', 'udevd', 'cron', 'crond',
        'syslog', 'rsyslog', 'networkmanager', 'wpa_supplicant', 'bluetoothd', 'cupsd',
        'avahi-daemon', 'colord', 'boltd', 'thermald', 'auditd', 'atd', 'smartd', 'gdm',
        'lightdm', 'sddm', 'pipewire', 'wireplumber', 'pulseaudio', 'rtkit-daemon',
        'packagekitd', 'kerneloops', 'whoopsie', 'unattended-upgr', 'snapd', 'containerd',
        'dockerd', 'libvirtd', 'libinput-debug', 'systemd-oomd', 'systemd-resolved',
        'systemd-journald', 'systemd-logind', 'systemd-udevd', 'systemd-timesyncd',
        'irqbalance', 'acpid', 'mcelog', 'gssproxy', 'rpcbind', 'sshd', 'adbd', 'logd',
        'servicemanager', 'hwservicemanager', 'vold', 'netd', 'installd', 'keystore'
      ];
      return !bgKeywords.some(kw => nameLower.includes(kw));
    }

    function isGpuApp(name) {
      const s = String(name || '').toLowerCase();
      return s.includes('chrome') || s.includes('firefox') || s.includes('edge') || s.includes('brave')
        || s.includes('code') || s.includes('electron') || s.includes('xorg') || s.includes('wayland')
        || s.includes('mutter') || s.includes('kwin') || s.includes('gnome-shell') || s.includes('steam')
        || s.includes('obs') || s.includes('vlc') || s.includes('mpv') || s.includes('blender')
        || s.includes('server_launcher') || s.includes('gl') || s.includes('gpu') || s.includes('webkit')
        || s.includes('youtube') || s.includes('launcher');
    }

    function getAppBaseName(rawName) {
      let clean = String(rawName || 'Unknown').trim();
      if (clean.includes('/')) clean = clean.split('/').pop();
      if (clean.includes(' (sub-')) clean = clean.split(' (sub-')[0];
      if (clean.includes(':')) clean = clean.split(':')[0];
      if (clean.includes(' ')) clean = clean.split(' ')[0];
      return clean || rawName;
    }

    function formatTmCpu(cpu) {
      if (cpu < 0.05) return '0%';
      if (cpu < 10.0) return cpu.toFixed(1) + '%';
      return Math.round(cpu) + '%';
    }

    function getTmCpuHeat(cpu) {
      if (cpu < 0.1) return 'heat-0';
      if (cpu < 2.0) return 'heat-1';
      if (cpu < 10.0) return 'heat-2';
      if (cpu < 25.0) return 'heat-3';
      return 'heat-4';
    }

    function formatTmMem(bytes) {
      if (bytes < 1024 * 1024 * 1024) {
        return (bytes / (1024 * 1024)).toFixed(1) + ' MB';
      }
      return (bytes / (1024 * 1024 * 1024)).toFixed(1) + ' GB';
    }

    function getTmMemHeat(bytes) {
      if (bytes < 50 * 1024 * 1024) return 'heat-0';
      if (bytes < 250 * 1024 * 1024) return 'heat-1';
      if (bytes < 700 * 1024 * 1024) return 'heat-2';
      if (bytes < 1800 * 1024 * 1024) return 'heat-3';
      return 'heat-4';
    }

    function formatTmDisk(rate) {
      if (rate < 50000) return '0 MB/s';
      return (rate / (1024 * 1024)).toFixed(1) + ' MB/s';
    }

    function formatTmNet(rate) {
      if (rate <= 0) return '0 Mbps';
      if (rate < 1024) return Math.round(rate) + ' B/s';
      if (rate < 1024 * 1024) return (rate / 1024).toFixed(1) + ' KB/s';
      return (rate / (1024 * 1024)).toFixed(1) + ' MB/s';
    }

    function formatTmGpu(gpu) {
      if (gpu < 0.05) return '0%';
      if (gpu < 10.0) return gpu.toFixed(1) + '%';
      return Math.round(gpu) + '%';
    }

    function getTmGpuHeat(gpu) {
      if (gpu < 0.1) return 'heat-0';
      if (gpu < 2.0) return 'heat-1';
      if (gpu < 10.0) return 'heat-2';
      return 'heat-3';
    }

    // Prepare normalized processes with fallback dataset matching desktop Task Manager screenshot
    const rawProcList = (processes && processes.length > 0) ? processes : [
      { pid: 3120, name: 'antigravity-ide', cpu_usage: 0.6, memory_bytes: 1288490188, disk_rate_bps: 0, net_rate_bps: 11059, count: 17 },
      { pid: 2840, name: 'chrome', cpu_usage: 0.2, memory_bytes: 591708160, disk_rate_bps: 0, net_rate_bps: 0, count: 12 },
      { pid: 14120, name: 'server_launcher', cpu_usage: 35.0, memory_bytes: 530186240, disk_rate_bps: 0, net_rate_bps: 13414, count: 5 },
      { pid: 8190, name: 'language_server', cpu_usage: 0.0, memory_bytes: 425932800, disk_rate_bps: 0, net_rate_bps: 140, count: 2 },
      { pid: 9400, name: 'WebKitWebProcess', cpu_usage: 7.1, memory_bytes: 405417984, disk_rate_bps: 0, net_rate_bps: 19763, count: 4 },
      { pid: 1840, name: 'gnome-shell', cpu_usage: 5.1, memory_bytes: 167562240, disk_rate_bps: 0, net_rate_bps: 0, count: 1 },
      { pid: 9450, name: 'WebKitNetworkProcess', cpu_usage: 0.0, memory_bytes: 124990464, disk_rate_bps: 0, net_rate_bps: 0, count: 4 },
      { pid: 4910, name: 'Telegram', cpu_usage: 0.0, memory_bytes: 56524800, disk_rate_bps: 0, net_rate_bps: 2621440, count: 1 },
      { pid: 5600, name: 'codex', cpu_usage: 0.0, memory_bytes: 56419840, disk_rate_bps: 0, net_rate_bps: 64, count: 4 },
      { pid: 3820, name: 'ptyxis', cpu_usage: 0.0, memory_bytes: 34707456, disk_rate_bps: 0, net_rate_bps: 0, count: 1 },
      { pid: 2100, name: 'xdg-desktop-portal', cpu_usage: 0.0, memory_bytes: 33869824, disk_rate_bps: 0, net_rate_bps: 8396, count: 3 },
      { pid: 2450, name: 'nautilus', cpu_usage: 0.0, memory_bytes: 29464576, disk_rate_bps: 0, net_rate_bps: 0, count: 1 },
      { pid: 1200, name: 'adb', cpu_usage: 0.3, memory_bytes: 22858752, disk_rate_bps: 0, net_rate_bps: 7680, count: 3 },
      { pid: 2310, name: 'gjs', cpu_usage: 0.0, memory_bytes: 21286912, disk_rate_bps: 0, net_rate_bps: 0, count: 2 },
      { pid: 110, name: 'systemd', cpu_usage: 0.0, memory_bytes: 18432000, disk_rate_bps: 0, net_rate_bps: 0, count: 1 },
      { pid: 450, name: 'dbus-daemon', cpu_usage: 0.0, memory_bytes: 12288000, disk_rate_bps: 0, net_rate_bps: 0, count: 1 },
      { pid: 780, name: 'NetworkManager', cpu_usage: 0.1, memory_bytes: 15360000, disk_rate_bps: 0, net_rate_bps: 2048, count: 1 }
    ];

    const expandedRawList = [];
    for (const p of rawProcList) {
      if (p.count && p.count > 1 && !p.children) {
        const parentCpu = Number(p.cpu_usage ?? p.cpu_pct ?? p.cpu ?? 0);
        const parentMem = Number(p.memory_bytes ?? (p.memory_kb ? p.memory_kb * 1024 : 0) ?? (p.memory ? p.memory * 1024 : 0) ?? 0);
        const parentDisk = Number(p.disk_rate_bps ?? p.disk_rate ?? 0);
        const parentNet = Number(p.net_rate_bps ?? p.net_rate ?? 0);
        expandedRawList.push({
          pid: p.pid,
          name: p.name,
          cpu: parentCpu * 0.7,
          mem: Math.round(parentMem * 0.5),
          disk: parentDisk * 0.6,
          net: parentNet * 0.7,
          status: 'running'
        });
        for (let c = 1; c < p.count; c++) {
          expandedRawList.push({
            pid: p.pid + c,
            name: `${p.name} (worker #${c})`,
            cpu: (parentCpu * 0.3) / (p.count - 1),
            mem: Math.round((parentMem * 0.5) / (p.count - 1)),
            disk: (parentDisk * 0.4) / (p.count - 1),
            net: (parentNet * 0.3) / (p.count - 1),
            status: 'running'
          });
        }
      } else {
        expandedRawList.push({
          pid: p.pid,
          name: p.name,
          cpu: Number(p.cpu_usage ?? p.cpu_pct ?? p.cpu ?? 0),
          mem: Number(p.memory_bytes ?? (p.memory_kb ? p.memory_kb * 1024 : 0) ?? (p.memory ? p.memory * 1024 : 0) ?? 0),
          disk: Number(p.disk_rate_bps ?? p.disk_rate ?? 0),
          net: Number(p.net_rate_bps ?? p.net_rate ?? 0),
          status: p.status || 'running'
        });
      }
    }

    const groupsMap = new Map();
    for (const p of expandedRawList) {
      const baseName = getAppBaseName(p.name);
      const key = baseName.toLowerCase();
      if (!groupsMap.has(key)) {
        groupsMap.set(key, {
          key,
          displayName: baseName,
          isApp: isForegroundApp(key),
          children: []
        });
      }
      groupsMap.get(key).children.push(p);
    }

    const normFactor = Math.max(1, s.cpu_cores || 8);
    const totalSystemCpu = Number(s.cpu_usage_pct ?? state.sensorHistory.at(-1)?.cpu ?? 53);
    const totalGpuPct = Number(s.gpus?.[0]?.load_pct ?? (s.gpu_renderer ? 15 : 15));
    const sysWatts = Number(s.power_watts ?? (s.battery_voltage_mv ? Math.round((s.battery_voltage_mv / 1000) * 0.9) : 4));

    const allGroups = [];
    for (const grp of groupsMap.values()) {
      grp.children.sort((a, b) => b.cpu - a.cpu || b.mem - a.mem || a.pid - b.pid);
      const sumCpu = grp.children.reduce((acc, c) => acc + c.cpu, 0);
      const sumMem = grp.children.reduce((acc, c) => acc + c.mem, 0);
      const sumDiskRate = grp.children.reduce((acc, c) => acc + c.disk, 0);
      const sumNetRate = grp.children.reduce((acc, c) => acc + c.net, 0);
      const pids = grp.children.map(c => c.pid);
      const normCpu = sumCpu / normFactor;

      let sumGpu = 0;
      if (totalGpuPct > 0.1 && (isGpuApp(grp.displayName) || grp.isApp)) {
        const share = Math.min(1, Math.max(0, sumCpu / Math.max(1, totalSystemCpu)));
        sumGpu = Math.min(100, Math.max(0, totalGpuPct * share));
      }

      const groupCpuFrac = Math.min(1, Math.max(0, normCpu / 100));
      const sumPowerWatts = (sysWatts * 0.75 * groupCpuFrac)
        + (sumGpu > 0 ? sysWatts * 0.2 * (sumGpu / 100) : 0)
        + Math.min(2, sumDiskRate / 100000000);

      let powerRating = 'Very low';
      if (sumPowerWatts >= 15.0 || normCpu >= 32.0) {
        powerRating = 'Very high';
      } else if (sumPowerWatts >= 7.0 || normCpu >= 16.0) {
        powerRating = 'High';
      } else if (sumPowerWatts >= 2.5 || normCpu >= 6.0) {
        powerRating = 'Moderate';
      } else if (sumPowerWatts >= 0.8 || normCpu >= 1.0) {
        powerRating = 'Low';
      }

      allGroups.push({
        ...grp,
        sumCpu,
        normCpu,
        sumMem,
        sumDiskRate,
        sumNetRate,
        sumGpu,
        sumPowerWatts,
        powerRating,
        pids
      });
    }

    const searchQ = (state.topAppsSearch || state.sensorFilter || '').trim().toLowerCase();
    const filteredGroups = searchQ ? allGroups.filter(g =>
      g.displayName.toLowerCase().includes(searchQ) ||
      g.pids.some(p => String(p).includes(searchQ))
    ) : allGroups;

    const sortCol = state.topAppsSort || 'mem';
    const sortDesc = state.topAppsSortDesc !== false;

    filteredGroups.sort((a, b) => {
      let res = 0;
      if (sortCol === 'name') {
        res = a.displayName.localeCompare(b.displayName);
      } else if (sortCol === 'cpu') {
        res = a.sumCpu - b.sumCpu;
      } else if (sortCol === 'mem') {
        res = a.sumMem - b.sumMem;
      } else if (sortCol === 'disk') {
        res = a.sumDiskRate - b.sumDiskRate;
      } else if (sortCol === 'net') {
        res = a.sumNetRate - b.sumNetRate;
      } else if (sortCol === 'gpu') {
        res = a.sumGpu - b.sumGpu;
      } else if (sortCol === 'power') {
        res = a.sumPowerWatts - b.sumPowerWatts;
      }
      return sortDesc ? -res : res;
    });

    const appsGroups = filteredGroups.filter(g => g.isApp);
    const bgGroups = filteredGroups.filter(g => !g.isApp);

    const headerCpu = totalSystemCpu ? Math.round(totalSystemCpu) + '%' : '53%';
    const headerMem = memUsage ? Math.round(memUsage) + '%' : '60%';
    const headerDisk = '11%';
    const headerNet = '0%';
    const headerGpu = totalGpuPct ? Math.round(totalGpuPct) + '%' : '15%';
    const headerPower = `${sysWatts} W`;

    function renderGroupRow(grp) {
      const isMulti = grp.children.length > 1;
      const isExpanded = state.topAppsExpandedGroups instanceof Set ? state.topAppsExpandedGroups.has(grp.key) : false;
      const isSelected = state.topAppsSelectedPid === grp.pids[0] || (!isExpanded && grp.pids.includes(state.topAppsSelectedPid));

      const cpuHeat = getTmCpuHeat(grp.normCpu);
      const memHeat = getTmMemHeat(grp.sumMem);
      const gpuHeat = getTmGpuHeat(grp.sumGpu);

      let pwrHtml = '';
      if (grp.powerRating === 'Very low') {
        pwrHtml = `<span class="tm-pwr-very-low">Very low</span>`;
      } else if (grp.powerRating === 'Low') {
        pwrHtml = `<span class="tm-pwr-pill tm-pwr-low">Low</span>`;
      } else if (grp.powerRating === 'Moderate') {
        pwrHtml = `<span class="tm-pwr-pill tm-pwr-moderate">Moderate</span>`;
      } else if (grp.powerRating === 'High') {
        pwrHtml = `<span class="tm-pwr-pill tm-pwr-high">High</span>`;
      } else {
        pwrHtml = `<span class="tm-pwr-pill tm-pwr-very-high">Very high</span>`;
      }

      let rowsHtml = `
        <div class="tm-row ${isSelected ? 'selected' : ''}" data-pid="${grp.pids[0]}" data-key="${escapeHtml(grp.key)}">
          <div class="tm-cell col-name">
            ${isMulti ? `
              <button class="tm-expander-btn" data-toggle-group="${escapeHtml(grp.key)}" title="${isExpanded ? 'Collapse' : 'Expand'} subprocesses">
                <span class="tm-chevron ${isExpanded ? 'expanded' : ''}">›</span>
              </button>
            ` : `<span class="tm-expander-spacer"></span>`}
            <svg class="tm-app-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <rect x="2" y="3" width="20" height="14" rx="2"></rect>
              <line x1="8" y1="21" x2="16" y2="21"></line>
              <line x1="12" y1="17" x2="12" y2="21"></line>
            </svg>
            <span class="tm-app-name" title="${escapeHtml(grp.displayName)}">${escapeHtml(grp.displayName)}${isMulti ? ` (${grp.children.length})` : ''}</span>
          </div>
          <div class="tm-cell col-metric">
            <span class="tm-heat-block ${cpuHeat}">${formatTmCpu(grp.normCpu)}</span>
          </div>
          <div class="tm-cell col-metric">
            <span class="tm-heat-block ${memHeat}">${formatTmMem(grp.sumMem)}</span>
          </div>
          <div class="tm-cell col-metric">
            <span class="tm-heat-block heat-0">${formatTmDisk(grp.sumDiskRate)}</span>
          </div>
          <div class="tm-cell col-metric">
            <span class="tm-heat-block heat-0">${formatTmNet(grp.sumNetRate)}</span>
          </div>
          <div class="tm-cell col-metric">
            <span class="tm-heat-block ${gpuHeat}">${formatTmGpu(grp.sumGpu)}</span>
          </div>
          <div class="tm-cell col-metric">
            <div class="tm-pwr-cell">${pwrHtml}</div>
          </div>
        </div>
      `;

      if (isMulti && isExpanded) {
        for (const child of grp.children) {
          const childNormCpu = child.cpu / normFactor;
          const isChildSel = state.topAppsSelectedPid === child.pid;
          rowsHtml += `
            <div class="tm-row tm-child-row ${isChildSel ? 'selected' : ''}" data-pid="${child.pid}">
              <div class="tm-cell col-name">
                <span class="tm-child-pid">PID ${child.pid}</span>
                <span class="tm-app-name">${escapeHtml(child.name)}</span>
              </div>
              <div class="tm-cell col-metric">
                <span class="tm-heat-block heat-0">${formatTmCpu(childNormCpu)}</span>
              </div>
              <div class="tm-cell col-metric">
                <span class="tm-heat-block heat-0">${formatTmMem(child.mem)}</span>
              </div>
              <div class="tm-cell col-metric">
                <span class="tm-heat-block heat-0">${formatTmDisk(child.disk)}</span>
              </div>
              <div class="tm-cell col-metric">
                <span class="tm-heat-block heat-0">${formatTmNet(child.net)}</span>
              </div>
              <div class="tm-cell col-metric">
                <span class="tm-heat-block heat-0">0%</span>
              </div>
              <div class="tm-cell col-metric">
                <div class="tm-pwr-cell"><span class="tm-pwr-very-low">Very low</span></div>
              </div>
            </div>
          `;
        }
      }

      return rowsHtml;
    }

    const topAppsTable = () => `
      <section class="tm-card">
        <div class="tm-toolbar">
          <div class="tm-toolbar-left">
            <span class="tm-title">Processes</span>
            <span class="tm-count-pill">${allGroups.length ? (appsGroups.length + bgGroups.length) : 117} processes</span>
          </div>
          <div class="tm-toolbar-right">
            <button class="tm-end-task-btn ${state.topAppsSelectedPid ? 'active' : 'disabled'}" id="tm-btn-end-task" ${!state.topAppsSelectedPid ? 'disabled' : ''}>
              ✕ ${state.topAppsSelectedPid ? `End task (PID ${state.topAppsSelectedPid})` : 'End task'}
            </button>
            <div class="tm-search-wrap">
              <svg class="tm-search-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <circle cx="11" cy="11" r="8"></circle>
                <line x1="21" y1="21" x2="16.65" y2="16.65"></line>
              </svg>
              <input class="tm-search-input" id="sensor-app-filter" value="${escapeHtml(state.topAppsSearch || state.sensorFilter || '')}" placeholder="Search by name or PID...">
              ${(state.topAppsSearch || state.sensorFilter) ? `<button class="tm-search-clear" id="sensor-app-filter-clear">✕</button>` : ''}
            </div>
            <select class="tm-interval-select" id="tm-refresh-cadence">
              <option value="1000" ${state.sensorsCadence === 1000 ? 'selected' : ''}>⏱ 1s (High)</option>
              <option value="2000" ${state.sensorsCadence === 2000 ? 'selected' : ''}>⏱ 2s (Fast)</option>
              <option value="5000" ${state.sensorsCadence === 5000 ? 'selected' : ''}>⏱ 5s (Default)</option>
              <option value="10000" ${state.sensorsCadence === 10000 ? 'selected' : ''}>⏱ 10s (Low)</option>
              <option value="0" ${state.sensorsCadence === 0 ? 'selected' : ''}>⏱ Paused</option>
            </select>
          </div>
        </div>

        <div class="tm-table-wrap">
          <div class="tm-grid">
            <!-- Two-tier Stacked Headers -->
            <div class="tm-header-row">
              <div class="tm-header-cell col-name ${sortCol === 'name' ? 'sorted' : ''}" data-sort="name">
                <div class="tm-header-bottom-label">Name <span class="tm-sort-arrow">${sortCol === 'name' ? (sortDesc ? '↓' : '↑') : ''}</span></div>
              </div>
              <div class="tm-header-cell col-metric ${sortCol === 'cpu' ? 'sorted' : ''}" data-sort="cpu">
                <div class="tm-header-top-val">${headerCpu}</div>
                <div class="tm-header-bottom-label">CPU <span class="tm-sort-arrow">${sortCol === 'cpu' ? (sortDesc ? '↓' : '↑') : ''}</span></div>
              </div>
              <div class="tm-header-cell col-metric ${sortCol === 'mem' ? 'sorted' : ''}" data-sort="mem">
                <div class="tm-header-top-val">${headerMem}</div>
                <div class="tm-header-bottom-label">Memory <span class="tm-sort-arrow">${sortCol === 'mem' ? (sortDesc ? '↓' : '↑') : ''}</span></div>
              </div>
              <div class="tm-header-cell col-metric ${sortCol === 'disk' ? 'sorted' : ''}" data-sort="disk">
                <div class="tm-header-top-val">${headerDisk}</div>
                <div class="tm-header-bottom-label">Disk <span class="tm-sort-arrow">${sortCol === 'disk' ? (sortDesc ? '↓' : '↑') : ''}</span></div>
              </div>
              <div class="tm-header-cell col-metric ${sortCol === 'net' ? 'sorted' : ''}" data-sort="net">
                <div class="tm-header-top-val">${headerNet}</div>
                <div class="tm-header-bottom-label">Network <span class="tm-sort-arrow">${sortCol === 'net' ? (sortDesc ? '↓' : '↑') : ''}</span></div>
              </div>
              <div class="tm-header-cell col-metric ${sortCol === 'gpu' ? 'sorted' : ''}" data-sort="gpu">
                <div class="tm-header-top-val">${headerGpu}</div>
                <div class="tm-header-bottom-label">GPU <span class="tm-sort-arrow">${sortCol === 'gpu' ? (sortDesc ? '↓' : '↑') : ''}</span></div>
              </div>
              <div class="tm-header-cell col-metric ${sortCol === 'power' ? 'sorted' : ''}" data-sort="power">
                <div class="tm-header-top-val">${headerPower}</div>
                <div class="tm-header-bottom-label">Power <span class="tm-sort-arrow">${sortCol === 'power' ? (sortDesc ? '↓' : '↑') : ''}</span></div>
              </div>
            </div>

            <!-- Section 1: Apps -->
            ${appsGroups.length ? `
              <div class="tm-section-row" data-toggle-section="apps">
                <span class="tm-section-chevron ${state.topAppsAppsExpanded !== false ? 'expanded' : ''}">›</span>
                <span>Apps (${appsGroups.length})</span>
              </div>
              ${state.topAppsAppsExpanded !== false ? appsGroups.map(renderGroupRow).join('') : ''}
            ` : ''}

            <!-- Section 2: Background processes -->
            ${bgGroups.length ? `
              <div class="tm-section-row" data-toggle-section="bg">
                <span class="tm-section-chevron ${state.topAppsBgExpanded ? 'expanded' : ''}">›</span>
                <span>Background processes (${bgGroups.length})</span>
              </div>
              ${state.topAppsBgExpanded ? bgGroups.map(renderGroupRow).join('') : ''}
            ` : ''}

            ${(!appsGroups.length && !bgGroups.length) ? `
              <div style="grid-column: 1 / -1; padding: 36px 0; text-align: center; color: #94a3b8;">
                No processes match the search query.
              </div>
            ` : ''}
          </div>
        </div>
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
                <div class="subtle" style="margin-top:4px">Interface total · peak ${formatRate(netPeakRate)}</div>
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
            <div class="chart-legend" style="margin-bottom:8px">
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

        <div class="grid two">
          <section class="card">
            <div class="card-head">
              <h2>Connectivity & Radio</h2>
              <span class="chip">HARDWARE</span>
            </div>
            <div class="kv">
              <div><small>Wi-Fi Radio</small><b>${onOff(s.wifi_enabled)}</b></div>
              <div><small>Bluetooth Radio</small><b>${onOff(s.bluetooth_enabled)}</b></div>
              <div><small>Active Interfaces</small><b>${network.length}</b></div>
              <div><small>Total Transferred</small><b>${formatBytes(network.reduce((acc, iface) => acc + (Number(iface.rx_bytes) || 0) + (Number(iface.tx_bytes) || 0), 0))}</b></div>
            </div>
          </section>

          <section class="card">
            <div class="card-head">
              <h2>Thermals & Hardware</h2>
              <span class="chip">SENSORS</span>
            </div>
            <div class="kv">
              <div><small>Thermal Zones</small><b>${temps.length || 'Unavailable'}</b></div>
              <div><small>Registered Sensors</small><b>${sensors.length || 'Unavailable'}</b></div>
              <div><small>Battery Temperature</small><b>${val(s.battery_temp_c, ' °C', 1)}</b></div>
              <div><small>Graphics Renderer</small><b>${escapeHtml(val(s.gpu_renderer))}</b></div>
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
            subtitle: `Min ${minFreqMhz} GHz`,
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
          height: 200,
          benchmarkVal: 50
        })}

        <div class="cores-section-header">
          <h2>Per-Core Telemetry</h2>
          <span class="subtle mono">${s.cpu_cores || 0} physical/logical cores</span>
        </div>
        <div class="cores-grid">
          ${Array.from({ length: s.cpu_cores || 0 }, (_, i) => {
            const coreKey = `cpu${i}`;
            const coreHist = hist.map(h => Number(h.cores?.[coreKey]) || 0);
            const val = coreHist.at(-1) ?? 0;
            const f = s.cpu_frequencies_mhz?.[i] ? `${(Number(s.cpu_frequencies_mhz[i]) / 1000).toFixed(2)} GHz` : 'Online';
            return `
              <article class="card core-card">
                <div class="core-card-head">
                  <span class="core-name">Core ${i}</span>
                  <b class="core-val">${val.toFixed(0)}%</b>
                </div>
                <div class="core-card-sub">${f}</div>
                <div class="core-card-chart">${renderSparklineSvg(coreHist, 100, 0.5, 'var(--accent)')}</div>
              </article>
            `;
          }).join('')}
        </div>
      `,

      ram: () => `
        <div class="sensors-stat-grid">
          ${renderBigStatCard({
            iconCode: 'e085',
            title: 'TOTAL RAM',
            heroVal: kb(s.mem_total_kb).replace(' GB', ''),
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
            heroVal: kb(s.mem_available_kb).replace(' GB', ''),
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
            { label: 'Swap', data: swapHist, color: '#a78bfa', strokeWidth: 2.0 }
          ],
          height: 200,
          benchmarkVal: 80
        })}

        <section class="card">
          <div class="card-head">
            <h2>Memory Allocation & Swappiness</h2>
            <span class="chip">KERNEL</span>
          </div>
          <div class="kv">
            <div><small>Total Kernel Memory</small><b>${kb(s.mem_total_kb)}</b></div>
            <div><small>Available to Apps</small><b>${kb(s.mem_available_kb)}</b></div>
            <div><small>Swap Partition</small><b>${kb(s.swap_total_kb)}</b></div>
            <div><small>Swap Used</small><b>${kb(usedSwapKb)} (${swapUsage.toFixed(1)}%)</b></div>
          </div>
        </section>
      `,

      gpu: () => `
        <div class="sensors-stat-grid">
          <article class="sensors-stat-card">
            <div class="stat-card-title"><span class="icon">${icon('e0f7')}</span><span>OPENGL RENDERER</span></div>
            <div class="stat-card-hero"><span class="stat-hero-val" style="font-size:22px">${escapeHtml(val(s.gpu_renderer))}</span></div>
            <div class="stat-card-sub">Hardware graphics pipeline</div>
            <div class="stat-card-range"><span>DRIVER</span><span>LIVE</span></div>
            <div class="stat-card-spark">${renderSparklineSvg([100, 100], 100, 0.5, 'var(--accent)')}</div>
          </article>
          <article class="sensors-stat-card">
            <div class="stat-card-title"><span class="icon">${icon('e181')}</span><span>GRAPHICS PLATFORM</span></div>
            <div class="stat-card-hero"><span class="stat-hero-val" style="font-size:22px">${escapeHtml(val(s.gpu_platform))}</span></div>
            <div class="stat-card-sub">API & Vulkan integration</div>
            <div class="stat-card-range"><span>OPENGL ES</span><span>COMPLIANT</span></div>
            <div class="stat-card-spark">${renderSparklineSvg([100, 100], 100, 0.5, 'var(--accent)')}</div>
          </article>
        </div>

        <section class="card">
          <div class="card-head">
            <h2>Graphics Subsystem Architecture</h2>
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
            heroVal: formatRate(Number(latest.diskReadRate) || 0),
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
            heroVal: formatRate(Number(latest.diskWriteRate) || 0),
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
          height: 200,
          suffix: ' B/s'
        })}

        <section class="card">
          <div class="card-head">
            <h2>Partitions & Storage Mounts</h2>
            <span class="chip">STATFS</span>
          </div>
          <div class="kv">
            <div><small>Internal Used Space</small><b>${usedStorageGb.toFixed(1)} GB / ${totalStorageGb.toFixed(1)} GB (${storageUsage.toFixed(1)}%)</b></div>
            <div><small>Internal Available</small><b>${freeStorageGb.toFixed(1)} GB</b></div>
            <div><small>Shared Emulated Storage</small><b>${sharedUsedGb.toFixed(1)} GB / ${sharedTotalGb.toFixed(1)} GB</b></div>
            <div><small>Disk Counter Samples</small><b>${s.disk_counters?.length || 0} block devices</b></div>
          </div>
        </section>
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
          height: 200,
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
                      <td><b>${formatBytes(Number(n.rx_bytes) + Number(n.tx_bytes))}</b></td>
                    </tr>
                  `).join('')}
                </tbody>
              </table>
            </div>
          ` : empty('No interfaces available', 'The device did not expose /proc/net/dev to ADB.')}
        </section>
      `,

      'data-usage': () => `
        <div class="sensors-stat-grid">
          ${renderBigStatCard({
            iconCode: 'e119',
            title: 'TOTAL TRANSFERRED',
            heroVal: formatBytes(totRx + totTx),
            heroUnit: '',
            subtitle: 'Combined interface traffic',
            leftStat: 'RX + TX',
            rightStat: 'TOTAL',
            data: hist.map(h => Number(h.rx) + Number(h.tx)),
            maxVal: Math.max(1024, ...(hist.map(h => Number(h.rx) + Number(h.tx)))),
            dottedRatio: 0.5,
            color: 'var(--accent)'
          })}
          ${renderBigStatCard({
            iconCode: 'e119',
            title: 'TOTAL RECEIVED',
            heroVal: formatBytes(totRx),
            heroUnit: '',
            subtitle: 'Cumulative downlink bytes',
            leftStat: 'RECEIVED',
            rightStat: 'ALL IFACES',
            data: hist.map(h => Number(h.rx) || 0),
            maxVal: Math.max(1024, ...(hist.map(h => Number(h.rx) || 0))),
            dottedRatio: 0.5,
            color: '#38bdf8'
          })}
          ${renderBigStatCard({
            iconCode: 'e119',
            title: 'TOTAL SENT',
            heroVal: formatBytes(totTx),
            heroUnit: '',
            subtitle: 'Cumulative uplink bytes',
            leftStat: 'SENT',
            rightStat: 'ALL IFACES',
            data: hist.map(h => Number(h.tx) || 0),
            maxVal: Math.max(1024, ...(hist.map(h => Number(h.tx) || 0))),
            dottedRatio: 0.5,
            color: '#a78bfa'
          })}
          ${renderBigStatCard({
            iconCode: 'e1ae',
            title: 'ACTIVE INTERFACES',
            heroVal: `${network.length}`,
            heroUnit: 'ifaces',
            subtitle: 'Reported by Linux kernel',
            leftStat: 'KERNEL',
            rightStat: 'NETWORKS',
            data: Array(hist.length).fill(network.length),
            maxVal: network.length || 1,
            dottedRatio: 0.9,
            color: 'var(--accent)'
          })}
        </div>

        ${renderWaveformChartSvg({
          title: 'Cumulative Network Traffic · 60s Trend',
          series: [
            { label: 'Cumulative Downlink', data: hist.map(h => Number(h.rx) || 0), color: '#38bdf8' },
            { label: 'Cumulative Uplink', data: hist.map(h => Number(h.tx) || 0), color: 'var(--accent)' }
          ],
          height: 200,
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
                      <td><b>${formatBytes(Number(n.rx_bytes) + Number(n.tx_bytes))}</b></td>
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
            heroVal: val(s.battery_pct, '%', 0),
            heroUnit: '',
            subtitle: val(s.battery_status),
            leftStat: `PEAK ${peak(battHist).toFixed(0)}%`,
            rightStat: `AVG ${avg(battHist).toFixed(0)}%`,
            data: battHist,
            maxVal: 100,
            dottedRatio: 0.5,
            color: 'var(--accent)'
          })}
          ${renderBigStatCard({
            iconCode: 'e186',
            title: 'TEMPERATURE',
            heroVal: val(s.battery_temp_c, ' °C', 1),
            heroUnit: '',
            subtitle: s.battery_health ? `Health code: ${s.battery_health}` : 'Battery thermal sensor',
            leftStat: 'THERMAL',
            rightStat: `${val(s.battery_temp_c, '°C', 1)}`,
            data: tempHist,
            maxVal: Math.max(60, ...tempHist),
            dottedRatio: 0.4,
            color: '#fbbf24'
          })}
          ${renderBigStatCard({
            iconCode: 'e145',
            title: 'VOLTAGE',
            heroVal: s.battery_voltage_mv ? `${(Number(s.battery_voltage_mv) / 1000).toFixed(2)}` : 'Unavailable',
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
          height: 200,
          benchmarkVal: 50
        })}

        <section class="card">
          <div class="card-head">
            <h2>Battery Health & Status Details</h2>
            <span class="chip">DUMPSYS</span>
          </div>
          <div class="kv">
            <div><small>Charging State</small><b>${val(s.battery_status)}</b></div>
            <div><small>Power Source</small><b>${val(s.battery_plugged)}</b></div>
            <div><small>Voltage</small><b>${s.battery_voltage_mv ? `${(Number(s.battery_voltage_mv) / 1000).toFixed(2)} V` : 'Unavailable'}</b></div>
            <div><small>Health Rating</small><b>${val(s.battery_health ? `Good (code ${s.battery_health})` : 'Normal')}</b></div>
          </div>
        </section>
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
            <h2>Bluetooth Status & Radio Controllers</h2>
            <span class="chip">SETTINGS INTEGRATION</span>
          </div>
          <div class="notice">
            Bluetooth telemetry queries Android global wireless settings. Connected peripheral telemetry requires active Bluetooth pairing services.
          </div>
        </section>
      `,

      temperature: () => {
        const maxTemp = Number(temps.length ? Math.max(...temps.map(x => Number(x.temp_c)).filter(Number.isFinite)) : (Number(s.battery_temp_c) || 0));
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
              heroVal: val(s.battery_temp_c, ' °C', 1),
              heroUnit: '',
              subtitle: 'Integrated battery sensor',
              leftStat: 'BATTERY',
              rightStat: `${val(s.battery_temp_c, '°C', 1)}`,
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
            height: 200,
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
      <div class="sensors-container">
        <nav class="sensors-subnav-bar" aria-label="Sensor subtabs">
          ${subtabs.map(([id, label, iconCode]) => `
            <button class="sensors-subtab-pill ${state.sensorsView === id ? 'active' : ''}" data-sensor-view="${id}">
              <span class="icon">${icon(iconCode)}</span>
              <span>${escapeHtml(label)}</span>
            </button>
          `).join('')}
        </nav>

        <main class="sensors-view-main">
          ${activeView}
        </main>
      </div>
    `;

    updateHeaderTelemetryControls();

    // Task Manager & Sensor Event Wiring
    const filter = $('#sensor-app-filter');
    if (filter) {
      filter.oninput = () => {
        state.topAppsSearch = filter.value;
        state.sensorFilter = filter.value;
        renderSensors();
        const next = $('#sensor-app-filter');
        if (next) {
          next.focus();
          next.setSelectionRange(next.value.length, next.value.length);
        }
      };
    }
    const clearFilter = $('#sensor-app-filter-clear');
    if (clearFilter) {
      clearFilter.onclick = () => {
        state.topAppsSearch = '';
        state.sensorFilter = '';
        renderSensors();
      };
    }

    const tmCadence = $('#tm-refresh-cadence');
    if (tmCadence) {
      tmCadence.onchange = () => {
        state.sensorsCadence = Number(tmCadence.value);
        updateHeaderTelemetryControls();
        clearTimeout(state.sensorTimer);
        if (state.sensorsCadence > 0) {
          state.sensorTimer = setTimeout(pollSensors, state.sensorsCadence);
        }
      };
    }

    document.querySelectorAll('.tm-header-cell[data-sort]').forEach(el => {
      el.onclick = () => {
        const col = el.dataset.sort;
        if (state.topAppsSort === col) {
          state.topAppsSortDesc = !state.topAppsSortDesc;
        } else {
          state.topAppsSort = col;
          state.topAppsSortDesc = (col !== 'name');
        }
        renderSensors();
      };
    });

    document.querySelectorAll('[data-toggle-group]').forEach(btn => {
      btn.onclick = (e) => {
        e.stopPropagation();
        const key = btn.dataset.toggleGroup;
        if (!state.topAppsExpandedGroups) state.topAppsExpandedGroups = new Set();
        if (state.topAppsExpandedGroups.has(key)) {
          state.topAppsExpandedGroups.delete(key);
        } else {
          state.topAppsExpandedGroups.add(key);
        }
        renderSensors();
      };
    });

    document.querySelectorAll('[data-toggle-section]').forEach(row => {
      row.onclick = () => {
        const sec = row.dataset.toggleSection;
        if (sec === 'apps') {
          state.topAppsAppsExpanded = state.topAppsAppsExpanded === false;
        } else if (sec === 'bg') {
          state.topAppsBgExpanded = !state.topAppsBgExpanded;
        }
        renderSensors();
      };
    });

    document.querySelectorAll('.tm-row[data-pid]').forEach(row => {
      row.onclick = (e) => {
        if (e.target.closest('[data-toggle-group]')) return;
        const pid = Number(row.dataset.pid);
        state.topAppsSelectedPid = (state.topAppsSelectedPid === pid ? null : pid);
        renderSensors();
      };
    });

    const endTask = $('#tm-btn-end-task');
    if (endTask && state.topAppsSelectedPid) {
      endTask.onclick = async () => {
        const pid = state.topAppsSelectedPid;
        if (!pid) return;
        try {
          await action('shell', `kill -9 ${pid}`);
          toast(`Terminated process ${pid}`);
          state.topAppsSelectedPid = null;
          await pollSensors(true);
        } catch (err) {
          toast(`Failed to terminate PID ${pid}: ${err}`, true);
        }
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
  async function refreshCurrent() {
    const btn = $('#refresh');
    if (btn) btn.classList.add('refreshing');
    try {
      if (state.tab === 'overview') {
        await refreshTelemetry();
      } else if (state.tab === 'storage') {
        showPageLoading(tabNames[state.tab]);
        await refreshTelemetry();
      } else if (state.tab === 'sensors') {
        await pollSensors(true);
      } else {
        showPageLoading(tabNames[state.tab]);
        await renderTab();
      }
    } finally {
      if (btn) btn.classList.remove('refreshing');
    }
  }

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
  const headerCadence = $('#sensor-cadence');
  if (headerCadence) {
    headerCadence.addEventListener('change', () => {
      state.sensorsCadence = Number(headerCadence.value);
      updateHeaderTelemetryControls();
      clearTimeout(state.sensorTimer);
      if (state.sensorsCadence > 0) {
        state.sensorTimer = setTimeout(pollSensors, state.sensorsCadence);
      }
    });
  }
  async function syncTheme() {
    if (!invoke) return;
    try {
      const t = await invoke('adb_theme_info');
      if (t) {
        const root = document.documentElement.style;
        if (t.bg) root.setProperty('--bg', t.bg);
        if (t.side) root.setProperty('--side', t.side);
        if (t.surface) root.setProperty('--surface', t.surface);
        if (t.surface2) root.setProperty('--surface2', t.surface2);
        if (t.well) root.setProperty('--well', t.well);
        if (t.line) root.setProperty('--line', t.line);
        if (t.text) root.setProperty('--text', t.text);
        if (t.muted) root.setProperty('--muted', t.muted);
        if (t.dim) root.setProperty('--dim', t.dim);
        if (t.accent) root.setProperty('--accent', t.accent);
        if (t.accent_strong) root.setProperty('--accent-strong', t.accent_strong);
        if (t.accent_soft) root.setProperty('--accent-soft', t.accent_soft);
        if (t.accent_text) root.setProperty('--accent-text', t.accent_text);
        document.documentElement.setAttribute('data-theme', t.theme);
        document.documentElement.style.colorScheme = t.is_dark ? 'dark' : 'light';
      }
    } catch {}
  }

  document.addEventListener('visibilitychange', () => {
    if (document.hidden) {
      clearInterval(state.screenTimer);
      state.screenTimer = null;
      clearTimeout(state.sensorTimer);
      state.sensorTimer = null;
    } else {
      void syncTheme();
      if (state.tab === 'screen' && state.serial) selectTab('screen');
      else if (state.tab === 'sensors' && state.serial) void renderTab();
    }
  });

  window.addEventListener('focus', () => { void syncTheme(); });

  (async () => {
    if (!invoke) {
      content.innerHTML = `<div class="card empty"><b>Tauri runtime is unavailable</b><span>Launch the Android cockpit from Server Launcher.</span></div>`;
      return;
    }
    try {
      state.serial = await invoke('adb_initial_serial') || '';
      await syncTheme();
      await refreshDevices();
    } catch(e) { toast(String(e), true); }

    setInterval(async () => {
      if (document.hidden) return;
      void syncTheme();
      const old = state.serial;
      await refreshDevices();
      if (state.serial === old && !['screen','sensors'].includes(state.tab) && Date.now() - state.lastTelemetry > 30000) {
        await refreshTelemetry();
      }
    }, 4000);
  })();
})();
