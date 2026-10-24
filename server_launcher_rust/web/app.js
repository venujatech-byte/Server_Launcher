// Server Launcher · Reactive Client Controller
(function() {
  'use strict';

  // Tauri 2 IPC Invoke Wrapper
  function invokeTauri(cmd, args = {}) {
    if (window.__TAURI__ && window.__TAURI__.core && typeof window.__TAURI__.core.invoke === 'function') {
      return window.__TAURI__.core.invoke(cmd, args);
    }
    return Promise.reject(new Error("Tauri IPC invoke is not available in browser"));
  }

  const state = {
    servers: [
      { key: 'vendly', name: 'Vendly Backend', group: 'backend', command: 'python run.py', port: 0, cwd: 'backend', status: 'stopped', own_console: false, autostart: false, auto_restart: false },
      { key: 'f', name: 'f', group: 'frontend', command: 'npm run dev', port: 5173, cwd: 'frontend', status: 'stopped', own_console: true, autostart: false, auto_restart: false },
      { key: 'portfolio', name: 'Portfolio', group: 'frontend', command: 'npm run dev', port: 3000, cwd: 'portfolio', status: 'stopped', own_console: true, autostart: false, auto_restart: false },
      { key: 'expo', name: 'React Native (Expo)', group: 'mobile', command: 'npx expo start', port: 8081, cwd: 'mobile', status: 'stopped', own_console: true, autostart: false, auto_restart: false },
      { key: 'analytics', name: 'Analytics Worker', group: 'backend', command: 'cargo run', port: 8080, cwd: 'analytics', status: 'stopped', own_console: false, autostart: false, auto_restart: false }
    ],
    scripts: [
      { id: 'deploy-staging', name: 'Deploy to Staging', shell: 'bash', cwd: '.', script: 'git push origin main && echo "Deployed to staging"', autostart: false },
      { id: 'db-migrate', name: 'Run DB Migrations', shell: 'bash', cwd: 'backend', script: 'python manage.py migrate', autostart: false },
      { id: 'docker-up', name: 'Compose Up Containers', shell: 'bash', cwd: '.', script: 'docker compose up -d', autostart: false }
    ],
    savedCommands: [
      { id: 'git-status', name: 'Git Status Concise', category: 'git', command: 'git status -s' },
      { id: 'docker-ps', name: 'List Running Containers', category: 'docker', command: 'docker ps --format "table {{.Names}}\t{{.Status}}\t{{.Ports}}"' },
      { id: 'free-mem', name: 'Memory Diagnostics', category: 'system', command: 'free -h' },
      { id: 'listening-ports', name: 'Listening TCP Sockets', category: 'general', command: 'ss -tulpn' }
    ],
    systemdServices: [
      { name: 'docker.service', description: 'Docker Application Container Engine', load_state: 'loaded', active_state: 'active', sub_state: 'running', is_active: true, is_failed: false },
      { name: 'nginx.service', description: 'A high performance web server', load_state: 'loaded', active_state: 'active', sub_state: 'running', is_active: true, is_failed: false },
      { name: 'postgresql.service', description: 'PostgreSQL database server', load_state: 'loaded', active_state: 'active', sub_state: 'running', is_active: true, is_failed: false },
      { name: 'redis.service', description: 'Advanced key-value store', load_state: 'loaded', active_state: 'inactive', sub_state: 'dead', is_active: false, is_failed: false }
    ],
    activeTab: 'servers',
    activeGroup: 'all',
    activeSubtab: 'overview',
    searchQuery: '',
    scanElapsedSec: 631,
    lanIp: '198.18.0.1'
  };

  // Cache DOM references
  const DOM = {
    btnMin: document.getElementById('btn-minimize'),
    btnMax: document.getElementById('btn-maximize'),
    btnClose: document.getElementById('btn-close'),
    windowTitlebar: document.getElementById('window-titlebar'),
    navTabs: document.querySelectorAll('.nav-pill-tab'),
    groupPillsContainer: document.getElementById('group-pills-container'),
    subtabs: document.querySelectorAll('.subtab-item'),
    globalSearch: document.getElementById('global-search'),
    subbarSearch: document.getElementById('subbar-search'),
    scanTimer: document.getElementById('scan-timer'),
    btnRefresh: document.getElementById('btn-refresh'),
    btnQuickLaunch: document.getElementById('btn-quick-launch'),
    btnAddServer: document.getElementById('btn-add-server'),
    btnAddGroup: document.getElementById('btn-add-group'),
    modalBackdrop: document.getElementById('modal-backdrop'),
    modalQuickLaunch: document.getElementById('modal-quick-launch'),
    modalAddServer: document.getElementById('modal-add-server'),
    modalAddGroup: document.getElementById('modal-add-group'),
    qlSearchInput: document.getElementById('ql-search-input'),
    qlResultsList: document.getElementById('ql-results-list'),
    formAddServer: document.getElementById('form-add-server'),
    btnCloseAddModal: document.getElementById('btn-close-add-modal'),
    btnCancelAdd: document.getElementById('btn-cancel-add'),
    btnCloseGroupModal: document.getElementById('btn-close-group-modal'),
    btnCancelGroup: document.getElementById('btn-cancel-group'),
    btnConfirmAddGroup: document.getElementById('btn-confirm-add-group'),
    inputNewGroup: document.getElementById('input-new-group'),
    colGroupedServers: document.getElementById('col-grouped-servers'),
    colManagedServers: document.getElementById('managed-cards-stack'),
    metaIp: document.getElementById('meta-ip'),
    metaCount: document.getElementById('meta-count'),
    overviewBadge: document.getElementById('overview-badge'),
    scriptsGrid: document.getElementById('scripts-grid'),
    cmdsGrid: document.getElementById('cmds-grid'),
    systemdTbody: document.getElementById('systemd-tbody'),
    btnRefreshSensors: document.getElementById('btn-refresh-sensors'),
    sensorCpuGlobal: document.getElementById('sensor-cpu-global'),
    meterCpuFill: document.getElementById('meter-cpu-fill'),
    sensorRamVal: document.getElementById('sensor-ram-val'),
    meterRamFill: document.getElementById('meter-ram-fill'),
    sensorCoresContainer: document.getElementById('sensor-cores-container')
  };

  // 1. Setup Window Control IPC (Minimize, Maximize, Close)
  function setupWindowControls() {
    if (DOM.btnMin) {
      DOM.btnMin.addEventListener('click', (e) => {
        e.stopPropagation();
        invokeTauri('app_minimize').catch(err => console.log('Minimize IPC:', err.message));
      });
    }

    if (DOM.btnMax) {
      DOM.btnMax.addEventListener('click', (e) => {
        e.stopPropagation();
        invokeTauri('app_toggle_maximize').catch(err => console.log('Maximize IPC:', err.message));
      });
    }

    if (DOM.btnClose) {
      DOM.btnClose.addEventListener('click', (e) => {
        e.stopPropagation();
        invokeTauri('app_close').catch(err => console.log('Close IPC:', err.message));
      });
    }

    // Double click header bar to toggle maximize
    if (DOM.windowTitlebar) {
      DOM.windowTitlebar.addEventListener('dblclick', (e) => {
        if (!e.target.closest('#titlebar-controls')) {
          invokeTauri('app_toggle_maximize').catch(err => console.log('Dblclick Maximize IPC:', err.message));
        }
      });
    }
  }

  // 2. Primary Navigation Tabs
  function setupNavTabs() {
    DOM.navTabs.forEach(tab => {
      tab.addEventListener('click', () => {
        DOM.navTabs.forEach(t => t.classList.remove('active'));
        tab.classList.add('active');
        state.activeTab = tab.getAttribute('data-tab');
        renderActiveView();
      });
    });
  }

  function renderActiveView() {
    const isServers = state.activeTab === 'servers';
    const workspace = document.getElementById('workspace-servers-layout');
    const groupBar = document.getElementById('group-filter-bar');

    if (workspace) workspace.style.display = isServers ? 'flex' : 'none';
    if (groupBar) groupBar.style.display = isServers ? 'flex' : 'none';

    ['scripts', 'cmds', 'systemd', 'sensors'].forEach(v => {
      const el = document.getElementById('view-' + v);
      if (el) el.style.display = (state.activeTab === v) ? 'flex' : 'none';
    });

    if (state.activeTab === 'scripts') renderScripts();
    if (state.activeTab === 'cmds') renderCmds();
    if (state.activeTab === 'systemd') renderSystemd();
    if (state.activeTab === 'sensors') renderSensors();
  }

  // 3. Dynamic Server Cards Rendering (Dual Column Layout)
  function renderServerCards() {
    if (!DOM.colGroupedServers || !DOM.colManagedServers) return;

    // Filter servers according to active group and search query
    const query = state.searchQuery.toLowerCase();
    const filtered = state.servers.filter(srv => {
      const matchesGroup = (state.activeGroup === 'all') || (srv.group.toLowerCase() === state.activeGroup.toLowerCase());
      const matchesQuery = !query || 
        srv.name.toLowerCase().includes(query) || 
        srv.command.toLowerCase().includes(query) || 
        (srv.port && srv.port.toString().includes(query)) ||
        srv.cwd.toLowerCase().includes(query);
      return matchesGroup && matchesQuery;
    });

    // Group servers by group
    const groups = {};
    filtered.forEach(srv => {
      const g = srv.group || 'General';
      if (!groups[g]) groups[g] = [];
      groups[g].push(srv);
    });

    // Render Left Column: Grouped Sections
    DOM.colGroupedServers.innerHTML = Object.keys(groups).map(gName => {
      const srvList = groups[gName];
      const gUpper = gName.toUpperCase();
      return `
        <div class="group-section" data-group-section="${gName.toLowerCase()}">
          <div class="group-header-row">
            <div class="group-title-wrap">
              <span class="icon">&#xe17f;</span>
              <span class="group-title-text">${gUpper} (${srvList.length})</span>
            </div>
            <div class="group-batch-actions">
              <button class="btn-batch btn-batch-start" data-batch-group="${gName}" data-action="batch-start">
                <span class="icon">&#xe13c;</span>
                <span>Start</span>
              </button>
              <button class="btn-batch btn-batch-stop" data-batch-group="${gName}" data-action="batch-stop">
                <span class="icon">&#xe167;</span>
                <span>Stop</span>
              </button>
            </div>
          </div>
          <div class="group-cards-list">
            ${srvList.map(srv => createServerCardHtml(srv, false)).join('')}
          </div>
        </div>
      `;
    }).join('');

    // Render Right Column: Launcher-managed servers
    DOM.colManagedServers.innerHTML = filtered.map(srv => createServerCardHtml(srv, true)).join('');

    updateOverviewStats();
  }

  function createServerCardHtml(srv, isManaged) {
    const isRunning = srv.status === 'running';
    const statusText = isRunning ? 'Running' : 'Stopped';
    const statusDotClass = isRunning ? 'status-dot running' : 'status-dot';
    const statusPillClass = isRunning ? 'status-pill running' : 'status-pill';
    const actionBtnClass = isRunning ? 'btn-action-stop' : 'btn-action-start';
    const actionIcon = isRunning ? '&#xe167;' : '&#xe13c;';
    const actionLabel = isRunning ? 'Stop' : 'Start';

    return `
      <div class="server-card" id="card-${srv.key}${isManaged ? '-managed' : ''}">
        <div class="card-top-row">
          <div class="card-title-group">
            <span class="${statusDotClass}"></span>
            <span class="card-name">${escapeHtml(srv.name)}</span>
          </div>
          <span class="${statusPillClass}">${statusText}</span>
        </div>
        
        <div class="command-well">
          <code class="code-text">${escapeHtml(srv.command)}</code>
          ${srv.port ? `<span class="port-right">:${srv.port}</span>` : ''}
        </div>
        
        <div class="card-tags-row">
          ${srv.port ? `
            <span class="tag-chip tag-port">
              <span class="icon">&#xe093;</span>
              <span>:${srv.port}</span>
            </span>` : ''}
          <span class="tag-chip tag-folder">
            <span class="icon">&#xe0d7;</span>
            <span>${escapeHtml(srv.cwd)}</span>
          </span>
        </div>
        
        <div class="card-actions-row">
          <button class="${actionBtnClass}" data-action="toggle-server" data-key="${srv.key}">
            <span class="icon">${actionIcon}</span>
            <span>${actionLabel}</span>
          </button>
          <div class="card-icon-actions">
            <button class="btn-icon" data-action="terminal" data-key="${srv.key}" title="Terminal"><span class="icon">&#xe181;</span></button>
            <button class="btn-icon" data-action="edit" data-key="${srv.key}" title="Edit"><span class="icon">&#xe1f9;</span></button>
            <button class="btn-icon" data-action="code" data-key="${srv.key}" title="Code"><span class="icon">&#xe093;</span></button>
            <button class="btn-icon" data-action="folder" data-key="${srv.key}" title="Folder"><span class="icon">&#xe0d7;</span></button>
            <button class="btn-icon btn-danger" data-action="delete" data-key="${srv.key}" title="Delete"><span class="icon">&#xe18e;</span></button>
          </div>
        </div>
        
        <div class="card-options-row">
          <label class="custom-checkbox">
            <input type="checkbox" name="autostart" data-key="${srv.key}" ${srv.autostart ? 'checked' : ''} />
            <span class="cb-box"></span>
            <span>autostart</span>
          </label>
          <label class="custom-checkbox">
            <input type="checkbox" name="external_terminal" data-key="${srv.key}" ${srv.own_console ? 'checked' : ''} />
            <span class="cb-box"></span>
            <span>run in external terminal</span>
          </label>
          <label class="custom-checkbox">
            <input type="checkbox" name="autorestart" data-key="${srv.key}" ${srv.auto_restart ? 'checked' : ''} />
            <span class="cb-box"></span>
            <span>auto-restart</span>
          </label>
        </div>
      </div>
    `;
  }

  // 4. Server Lifecycle Actions (Start, Stop, Terminal, Folder)
  function toggleServerStatus(key) {
    const srv = state.servers.find(s => s.key === key);
    if (!srv) return;

    if (srv.status === 'running') {
      invokeTauri('launcher_stop_server', { key }).then(() => {
        showToast(`Stopped ${srv.name}`);
      }).catch(() => {});
      srv.status = 'stopped';
    } else {
      invokeTauri('launcher_start_server', { key }).then(() => {
        showToast(`Started ${srv.name}`);
      }).catch(() => {});
      srv.status = 'running';
    }

    renderServerCards();
  }

  // 5. Group Filter & Subtabs Bar Setup
  function setupGroupFilter() {
    if (!DOM.groupPillsContainer) return;
    DOM.groupPillsContainer.addEventListener('click', (e) => {
      const pill = e.target.closest('.group-pill');
      if (pill) {
        document.querySelectorAll('.group-pill').forEach(p => p.classList.remove('active'));
        pill.classList.add('active');
        state.activeGroup = pill.getAttribute('data-group') || 'all';
        renderServerCards();
      }
    });

    if (DOM.btnAddGroup) {
      DOM.btnAddGroup.addEventListener('click', () => {
        openModal(DOM.modalAddGroup);
      });
    }

    if (DOM.btnConfirmAddGroup && DOM.inputNewGroup) {
      DOM.btnConfirmAddGroup.addEventListener('click', () => {
        const val = DOM.inputNewGroup.value.trim();
        if (val) {
          const btn = document.createElement('button');
          btn.className = 'group-pill';
          btn.setAttribute('data-group', val.toLowerCase());
          btn.textContent = val;
          DOM.groupPillsContainer.insertBefore(btn, DOM.btnAddGroup);
          DOM.inputNewGroup.value = '';
          closeModal(DOM.modalAddGroup);
          showToast(`Group '${val}' added`);
        }
      });
    }
  }

  function setupSubtabs() {
    DOM.subtabs.forEach(subtab => {
      subtab.addEventListener('click', () => {
        DOM.subtabs.forEach(s => s.classList.remove('active'));
        subtab.classList.add('active');
        state.activeSubtab = subtab.getAttribute('data-subtab');
      });
    });
  }

  // 6. Search & Keyboard Shortcuts (Ctrl+K, Ctrl+P, Ctrl+F, Esc)
  function setupSearch() {
    const onSearch = (e) => {
      state.searchQuery = e.target.value.trim();
      renderServerCards();
    };

    if (DOM.globalSearch) DOM.globalSearch.addEventListener('input', onSearch);
    if (DOM.subbarSearch) DOM.subbarSearch.addEventListener('input', onSearch);

    window.addEventListener('keydown', (e) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        DOM.globalSearch && DOM.globalSearch.focus();
      }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'f') {
        e.preventDefault();
        DOM.subbarSearch && DOM.subbarSearch.focus();
      }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'p') {
        e.preventDefault();
        openQuickLaunch();
      }
      if (e.key === 'Escape') {
        closeAllModals();
      }
    });
  }

  // 7. Quick Launch Command Palette (Ctrl+P)
  function openQuickLaunch() {
    openModal(DOM.modalQuickLaunch);
    if (DOM.qlSearchInput) {
      DOM.qlSearchInput.value = '';
      DOM.qlSearchInput.focus();
      renderQuickLaunchResults('');
    }
  }

  function renderQuickLaunchResults(query) {
    if (!DOM.qlResultsList) return;
    const q = query.toLowerCase();
    const results = [];

    state.servers.forEach(s => {
      if (!q || s.name.toLowerCase().includes(q) || s.command.toLowerCase().includes(q)) {
        results.push({
          title: `Start Server: ${s.name}`,
          badge: s.port ? `:${s.port}` : s.command,
          action: () => toggleServerStatus(s.key)
        });
      }
    });

    state.scripts.forEach(sc => {
      if (!q || sc.name.toLowerCase().includes(q)) {
        results.push({
          title: `Run Script: ${sc.name}`,
          badge: sc.shell,
          action: () => runScript(sc.id)
        });
      }
    });

    state.savedCommands.forEach(cmd => {
      if (!q || cmd.name.toLowerCase().includes(q) || cmd.command.toLowerCase().includes(q)) {
        results.push({
          title: `Run Command: ${cmd.name}`,
          badge: cmd.category,
          action: () => runCommand(cmd.command)
        });
      }
    });

    DOM.qlResultsList.innerHTML = results.map((r, i) => `
      <div class="ql-result-item ${i === 0 ? 'selected' : ''}" data-idx="${i}">
        <span class="icon">&#xe13c;</span>
        <span class="ql-title">${escapeHtml(r.title)}</span>
        <span class="ql-badge">${escapeHtml(r.badge)}</span>
      </div>
    `).join('');

    DOM.qlResultsList.querySelectorAll('.ql-result-item').forEach(item => {
      item.addEventListener('click', () => {
        const idx = parseInt(item.getAttribute('data-idx'), 10);
        if (results[idx]) {
          results[idx].action();
          closeModal(DOM.modalQuickLaunch);
        }
      });
    });
  }

  // 8. Add Server Form Handling
  function setupAddServerForm() {
    if (DOM.btnAddServer) {
      DOM.btnAddServer.addEventListener('click', () => openModal(DOM.modalAddServer));
    }

    if (DOM.formAddServer) {
      DOM.formAddServer.addEventListener('submit', (e) => {
        e.preventDefault();
        const fd = new FormData(DOM.formAddServer);
        const name = fd.get('name').trim();
        const command = fd.get('command').trim();
        const cwd = fd.get('cwd').trim() || '.';
        const port = parseInt(fd.get('port'), 10) || 0;
        const group = fd.get('group') || 'General';
        const key = name.toLowerCase().replace(/[^a-z0-9]/g, '_');

        const newSrv = {
          key,
          name,
          command,
          cwd,
          port,
          group,
          status: 'stopped',
          own_console: true,
          autostart: false,
          auto_restart: false
        };

        state.servers.push(newSrv);
        invokeTauri('launcher_save_server', { server: newSrv }).catch(() => {});
        renderServerCards();
        closeModal(DOM.modalAddServer);
        DOM.formAddServer.reset();
        showToast(`Server '${name}' created successfully`);
      });
    }
  }

  // 9. Scripts, Cmds, SystemD, Sensors Views Renderers
  function renderScripts() {
    if (!DOM.scriptsGrid) return;
    DOM.scriptsGrid.innerHTML = state.scripts.map(sc => `
      <div class="script-card">
        <div class="script-card-header">
          <span class="script-title">${escapeHtml(sc.name)}</span>
          <span class="shell-badge">${escapeHtml(sc.shell)}</span>
        </div>
        <div class="command-well">
          <code class="code-text">${escapeHtml(sc.script)}</code>
        </div>
        <div class="card-actions-row">
          <button class="btn-action-start" data-script-id="${sc.id}">
            <span class="icon">&#xe13c;</span>
            <span>Run Script</span>
          </button>
        </div>
      </div>
    `).join('');

    DOM.scriptsGrid.querySelectorAll('[data-script-id]').forEach(btn => {
      btn.addEventListener('click', () => {
        const id = btn.getAttribute('data-script-id');
        runScript(id);
      });
    });
  }

  function runScript(id) {
    const sc = state.scripts.find(s => s.id === id);
    if (!sc) return;
    showToast(`Executing script: ${sc.name}...`);
    const panel = document.getElementById('scripts-output-panel');
    const log = document.getElementById('scripts-output-log');
    if (panel) panel.style.display = 'flex';
    if (log) log.textContent = `Running ${sc.name} (${sc.shell})...\n`;

    invokeTauri('launcher_execute_script', { id }).then(out => {
      if (log) log.textContent += out + '\n[Process exited successfully with code 0]';
    }).catch(err => {
      if (log) log.textContent += `Error: ${err}\n[Process exited with non-zero error]`;
    });
  }

  function renderCmds() {
    if (!DOM.cmdsGrid) return;
    DOM.cmdsGrid.innerHTML = state.savedCommands.map(cmd => `
      <div class="cmd-card">
        <div class="cmd-card-header">
          <span class="cmd-title">${escapeHtml(cmd.name)}</span>
          <span class="shell-badge">${escapeHtml(cmd.category)}</span>
        </div>
        <div class="command-well">
          <code class="code-text">${escapeHtml(cmd.command)}</code>
        </div>
        <div class="card-actions-row">
          <button class="btn-action-start" data-cmd="${escapeHtml(cmd.command)}">
            <span class="icon">&#xe13c;</span>
            <span>Execute</span>
          </button>
        </div>
      </div>
    `).join('');

    DOM.cmdsGrid.querySelectorAll('[data-cmd]').forEach(btn => {
      btn.addEventListener('click', () => {
        const c = btn.getAttribute('data-cmd');
        runCommand(c);
      });
    });
  }

  function runCommand(cmdStr) {
    showToast(`Running: ${cmdStr}`);
    const out = document.getElementById('cmd-terminal-output');
    const status = document.getElementById('cmd-exec-status');
    if (status) status.textContent = 'Running...';
    if (out) out.textContent = `$ ${cmdStr}\n`;

    invokeTauri('launcher_execute_command', { command: cmdStr, cwd: null }).then(res => {
      if (out) out.textContent += res;
      if (status) status.textContent = 'Completed (0)';
    }).catch(err => {
      if (out) out.textContent += `\nError: ${err}`;
      if (status) status.textContent = 'Failed';
    });
  }

  function renderSystemd() {
    if (!DOM.systemdTbody) return;
    invokeTauri('launcher_list_systemd').then(list => {
      if (Array.isArray(list) && list.length > 0) {
        state.systemdServices = list;
      }
      populateSystemdTable();
    }).catch(() => {
      populateSystemdTable();
    });
  }

  function populateSystemdTable() {
    if (!DOM.systemdTbody) return;
    DOM.systemdTbody.innerHTML = state.systemdServices.map(s => {
      const chipClass = s.is_active ? 'chip-active' : (s.is_failed ? 'chip-failed' : 'chip-inactive');
      return `
        <tr>
          <td style="font-family: var(--font-mono); font-weight: 600;">${escapeHtml(s.name)}</td>
          <td style="color: var(--text-secondary);">${escapeHtml(s.description || 'System service')}</td>
          <td>${escapeHtml(s.load_state || 'loaded')}</td>
          <td><span class="chip-status ${chipClass}">${escapeHtml(s.active_state)}</span></td>
          <td style="color: var(--text-muted);">${escapeHtml(s.sub_state)}</td>
          <td>
            <button class="btn-icon" data-unit="${s.name}" data-sys-action="restart" title="Restart Service">
              <span class="icon">&#xe145;</span>
            </button>
          </td>
        </tr>
      `;
    }).join('');
  }

  function renderSensors() {
    invokeTauri('launcher_get_sensors').then(data => {
      if (data && DOM.sensorCpuGlobal) {
        DOM.sensorCpuGlobal.textContent = `${data.cpu_global.toFixed(1)}%`;
        if (DOM.meterCpuFill) DOM.meterCpuFill.style.width = `${data.cpu_global}%`;
        if (DOM.sensorRamVal) {
          DOM.sensorRamVal.textContent = `${(data.memory_used_mb / 1024).toFixed(1)} / ${(data.memory_total_mb / 1024).toFixed(1)} GB (${data.memory_pct.toFixed(0)}%)`;
        }
        if (DOM.meterRamFill) DOM.meterRamFill.style.width = `${data.memory_pct}%`;
        if (DOM.sensorCoresContainer && Array.isArray(data.cpu_cores)) {
          DOM.sensorCoresContainer.innerHTML = data.cpu_cores.map((c, i) => `
            <div class="core-cell">C${i}: ${c.toFixed(0)}%</div>
          `).join('');
        }
      }
    }).catch(() => {});
  }

  // 10. Live Timers & Initial Backend Synchronization
  function setupScanTimer() {
    setInterval(() => {
      state.scanElapsedSec++;
      const hrs = String(Math.floor(state.scanElapsedSec / 3600)).padStart(2, '0');
      const mins = String(Math.floor((state.scanElapsedSec % 3600) / 60)).padStart(2, '0');
      const secs = String(state.scanElapsedSec % 60).padStart(2, '0');
      if (DOM.scanTimer) {
        DOM.scanTimer.textContent = `${hrs}:${mins}:${secs}`;
      }
    }, 1000);

    if (DOM.btnRefresh) {
      DOM.btnRefresh.addEventListener('click', () => {
        state.scanElapsedSec = 0;
        showToast('Scanning network listeners & ports...');
        invokeTauri('launcher_scan_listeners').then(res => {
          showToast(`Port scan complete. Found ${res.listeners.length} active sockets.`);
        }).catch(() => {});
      });
    }
  }

  function syncWithBackend() {
    invokeTauri('launcher_get_data').then(data => {
      if (data) {
        if (data.lan_ip) {
          state.lanIp = data.lan_ip;
          if (DOM.metaIp) DOM.metaIp.textContent = data.lan_ip;
        }
        if (Array.isArray(data.servers) && data.servers.length > 0) {
          state.servers = data.servers.map(s => ({
            ...s,
            status: s.status || 'stopped'
          }));
        }
        if (Array.isArray(data.scripts) && data.scripts.length > 0) {
          state.scripts = data.scripts;
        }
        if (Array.isArray(data.saved_commands) && data.saved_commands.length > 0) {
          state.savedCommands = data.saved_commands;
        }
        renderServerCards();
      }
    }).catch(err => {
      console.log('Running with built-in state:', err.message);
      renderServerCards();
    });
  }

  function updateOverviewStats() {
    const running = state.servers.filter(s => s.status === 'running').length;
    const total = state.servers.length;
    if (DOM.overviewBadge) DOM.overviewBadge.textContent = `${running}/${total}`;
    if (DOM.metaCount) DOM.metaCount.textContent = total.toString();
  }

  // Modal helpers
  function openModal(el) {
    if (DOM.modalBackdrop) DOM.modalBackdrop.style.display = 'flex';
    if (el) el.style.display = 'flex';
  }

  function closeModal(el) {
    if (el) el.style.display = 'none';
    if (DOM.modalBackdrop) DOM.modalBackdrop.style.display = 'none';
  }

  function closeAllModals() {
    if (DOM.modalBackdrop) DOM.modalBackdrop.style.display = 'none';
    if (DOM.modalQuickLaunch) DOM.modalQuickLaunch.style.display = 'none';
    if (DOM.modalAddServer) DOM.modalAddServer.style.display = 'none';
    if (DOM.modalAddGroup) DOM.modalAddGroup.style.display = 'none';
  }

  function escapeHtml(str) {
    return String(str || '')
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;');
  }

  // Global event delegation for buttons
  function setupActionDelegation() {
    document.addEventListener('click', (e) => {
      const toggleBtn = e.target.closest('[data-action="toggle-server"]');
      if (toggleBtn) {
        const key = toggleBtn.getAttribute('data-key');
        toggleServerStatus(key);
        return;
      }

      const termBtn = e.target.closest('[data-action="terminal"]');
      if (termBtn) {
        const key = termBtn.getAttribute('data-key');
        const s = state.servers.find(srv => srv.key === key);
        if (s) invokeTauri('launcher_open_terminal', { cwd: s.cwd, command: s.command });
        return;
      }

      const folderBtn = e.target.closest('[data-action="folder"]');
      if (folderBtn) {
        const key = folderBtn.getAttribute('data-key');
        const s = state.servers.find(srv => srv.key === key);
        if (s) invokeTauri('launcher_open_folder', { path: s.cwd });
        return;
      }

      const codeBtn = e.target.closest('[data-action="code"]');
      if (codeBtn) {
        const key = codeBtn.getAttribute('data-key');
        const s = state.servers.find(srv => srv.key === key);
        if (s && s.port) {
          invokeTauri('launcher_open_browser', { url: `http://localhost:${s.port}` });
        } else if (s) {
          invokeTauri('launcher_open_terminal', { cwd: s.cwd, command: 'code .' });
        }
        return;
      }

      const deleteBtn = e.target.closest('[data-action="delete"]');
      if (deleteBtn) {
        const key = deleteBtn.getAttribute('data-key');
        if (confirm('Are you sure you want to delete this server configuration?')) {
          state.servers = state.servers.filter(s => s.key !== key);
          invokeTauri('launcher_delete_server', { key }).catch(() => {});
          renderServerCards();
          showToast('Server removed');
        }
        return;
      }
    });

    if (DOM.btnCloseAddModal) DOM.btnCloseAddModal.addEventListener('click', () => closeModal(DOM.modalAddServer));
    if (DOM.btnCancelAdd) DOM.btnCancelAdd.addEventListener('click', () => closeModal(DOM.modalAddServer));
    if (DOM.btnCloseGroupModal) DOM.btnCloseGroupModal.addEventListener('click', () => closeModal(DOM.modalAddGroup));
    if (DOM.btnCancelGroup) DOM.btnCancelGroup.addEventListener('click', () => closeModal(DOM.modalAddGroup));
    if (DOM.btnQuickLaunch) DOM.btnQuickLaunch.addEventListener('click', openQuickLaunch);
    if (DOM.modalBackdrop) {
      DOM.modalBackdrop.addEventListener('click', (e) => {
        if (e.target === DOM.modalBackdrop) closeAllModals();
      });
    }
  }

  function init() {
    setupWindowControls();
    setupNavTabs();
    setupGroupFilter();
    setupSubtabs();
    setupSearch();
    setupAddServerForm();
    setupScanTimer();
    setupActionDelegation();
    syncWithBackend();
    console.log('Server Launcher client ready.');
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
})();

// Toast Notification
function showToast(msg) {
  let t = document.getElementById('app-toast');
  if (!t) {
    t = document.createElement('div');
    t.id = 'app-toast';
    t.style.cssText = 'position:fixed;bottom:20px;right:20px;background:#162032;border:none;color:#f8fafc;padding:10px 18px;border-radius:8px;font-size:13px;font-weight:600;z-index:99999;box-shadow:0 8px 24px rgba(0,0,0,0.6);transition:opacity 0.25s;';
    document.body.appendChild(t);
  }
  t.textContent = msg;
  t.style.opacity = '1';
  clearTimeout(t._timer);
  t._timer = setTimeout(() => { t.style.opacity = '0'; }, 2600);
}

// Window blur handler
window.addEventListener('blur', () => { const tb = document.getElementById('window-titlebar'); if (tb) tb.classList.add('window-inactive'); });

// Window focus handler
window.addEventListener('focus', () => { const tb = document.getElementById('window-titlebar'); if (tb) tb.classList.remove('window-inactive'); });

// Copy IP handler
document.getElementById('meta-ip')?.addEventListener('click', () => { navigator.clipboard?.writeText(document.getElementById('meta-ip').textContent); showToast('Copied LAN IP to clipboard'); });

// Device rail switching handler
document.querySelectorAll('.rail-item').forEach(item => { item.addEventListener('click', () => { if (item.id === 'rail-export') { showToast('Configuration exported to servers.json'); return; } document.querySelectorAll('.rail-item').forEach(r => r.classList.remove('active')); item.classList.add('active'); showToast(`Connected machine: ${item.getAttribute('title') || 'Device'}`); }); });

// Command well copy handler
document.addEventListener('click', (e) => { const well = e.target.closest('.command-well'); if (well) { const code = well.querySelector('.code-text')?.textContent; if (code) { navigator.clipboard?.writeText(code); showToast(`Copied command: ${code}`); } } });

// Checkbox change handler
document.addEventListener('change', (e) => { if (e.target.matches('.custom-checkbox input')) { const name = e.target.getAttribute('name'); const checked = e.target.checked; const key = e.target.getAttribute('data-key'); showToast(`Updated ${name} to ${checked} for server`); } });

// Batch action listener
document.addEventListener('click', (e) => { const batch = e.target.closest('.btn-batch'); if (batch) { const group = batch.getAttribute('data-batch-group'); const action = batch.getAttribute('data-action'); showToast(`${action === 'batch-start' ? 'Starting' : 'Stopping'} all ${group} servers`); } });

// Clear script output handler
document.getElementById('btn-clear-script-output')?.addEventListener('click', () => { const panel = document.getElementById('scripts-output-panel'); if (panel) panel.style.display = 'none'; showToast('Script output cleared'); });

// Cmd category filter
document.getElementById('cmds-categories-bar')?.addEventListener('click', (e) => { const pill = e.target.closest('.group-pill'); if (pill) { document.querySelectorAll('#cmds-categories-bar .group-pill').forEach(p => p.classList.remove('active')); pill.classList.add('active'); showToast(`Filter commands: ${pill.textContent}`); } });
