// Server Launcher · Unified Client Controller
(function() {
  'use strict';

  function invokeTauri(cmd, args = {}) {
    if (window.__TAURI__ && window.__TAURI__.core && typeof window.__TAURI__.core.invoke === 'function') {
      return window.__TAURI__.core.invoke(cmd, args);
    }
    return Promise.reject(new Error('Tauri core invoke not available'));
  }

  // 1-to-1 exact dataset matching servers.json
  const state = {
    servers: [
      {
        key: 'f',
        name: 'f',
        cwd: '/home/venujasilva/Documents/Coding/Vendly.lk/frontend',
        command: 'npm run dev',
        port: 5173,
        group: 'Frontend',
        status: 'stopped',
        own_console: false,
        autostart: false,
        auto_restart: false,
        actions: [],
        links: []
      },
      {
        key: 'vendly_backend',
        name: 'Vendly Backend',
        cwd: '/home/venujasilva/Documents/Coding/Vendly.lk/backend',
        command: 'python run.py',
        port: 0,
        group: 'Backend',
        status: 'stopped',
        own_console: false,
        autostart: false,
        auto_restart: false,
        actions: [],
        links: []
      },
      {
        key: 'react_native__expo',
        name: 'React Native (Expo)',
        cwd: '/home/venujasilva/Documents/Coding/Vendly.lk-app',
        command: 'npx expo start',
        port: 8081,
        group: 'Mobile',
        status: 'stopped',
        own_console: false,
        autostart: false,
        auto_restart: false,
        actions: [
          { label: 'Android', command: 'npx expo run:android' },
          { label: 'iOS', command: 'npx expo run:ios' },
          { label: 'Web', command: 'npx expo start --web' },
          { label: 'Install', command: 'npx expo install' }
        ],
        links: [
          { label: 'Metro Bundler', url: 'http://localhost:8081' }
        ]
      }
    ],
    activeTab: 'servers',
    activeSubtab: 'overview',
    activeGroup: 'all',
    searchQuery: '',
    scanElapsedSec: 3209,
    lanIp: '192.168.1.5'
  };

  const DOM = {
    btnMin: document.getElementById('btn-minimize'),
    btnMax: document.getElementById('btn-maximize'),
    btnClose: document.getElementById('btn-close'),
    titlebar: document.getElementById('window-titlebar'),
    navTabs: document.querySelectorAll('.nav-pill-tab'),
    groupPillsContainer: document.getElementById('group-pills-container'),
    subtabsContainer: document.getElementById('subtabs-bar'),
    overviewContainer: document.getElementById('overview-view-container'),
    serverTabView: document.getElementById('server-tab-view'),
    globalSearch: document.getElementById('global-search'),
    subbarSearch: document.getElementById('subbar-search'),
    scanTimer: document.getElementById('scan-timer'),
    btnRefresh: document.getElementById('btn-refresh'),
    btnQuickLaunch: document.getElementById('btn-quick-launch'),
    btnAddServer: document.getElementById('btn-add-server'),
    btnSettings: document.getElementById('btn-settings'),
    modalBackdrop: document.getElementById('modal-backdrop'),
    modalQuickLaunch: document.getElementById('modal-quick-launch'),
    modalAddServer: document.getElementById('modal-add-server'),
    modalAddGroup: document.getElementById('modal-add-group'),
    modalSettings: document.getElementById('modal-settings'),
    qlSearchInput: document.getElementById('ql-search-input'),
    qlResultsList: document.getElementById('ql-results-list'),
    formAddServer: document.getElementById('form-add-server'),
    metaIp: document.getElementById('meta-ip'),
    metaCount: document.getElementById('meta-count'),
    overviewBadge: document.getElementById('overview-badge'),
    colGrouped: document.getElementById('col-grouped-servers'),
    colManaged: document.getElementById('managed-cards-stack'),
    tabSrvDot: document.getElementById('tab-srv-dot'),
    tabSrvTitle: document.getElementById('tab-srv-title'),
    tabSrvBadge: document.getElementById('tab-srv-badge'),
    tabSrvCwd: document.getElementById('tab-srv-cwd'),
    tabLogBody: document.getElementById('tab-log-body'),
    tabBtnToggle: document.getElementById('tab-btn-toggle'),
    tabBtnTerminal: document.getElementById('tab-btn-terminal'),
    tabBtnFolder: document.getElementById('tab-btn-folder'),
    tabBtnCode: document.getElementById('tab-btn-code'),
    tabCustomActionsBar: document.getElementById('tab-custom-actions-bar')
  };

  // 1. Window Controls IPC
  function setupWindowControls() {
    if (DOM.btnMin) {
      DOM.btnMin.addEventListener('click', (e) => {
        e.stopPropagation();
        invokeTauri('app_minimize').catch(() => {});
      });
    }
    if (DOM.btnMax) {
      DOM.btnMax.addEventListener('click', (e) => {
        e.stopPropagation();
        invokeTauri('app_toggle_maximize').catch(() => {});
      });
    }
    if (DOM.btnClose) {
      DOM.btnClose.addEventListener('click', (e) => {
        e.stopPropagation();
        invokeTauri('app_close').catch(() => {});
      });
    }
    if (DOM.titlebar) {
      DOM.titlebar.addEventListener('dblclick', (e) => {
        if (!e.target.closest('#titlebar-controls')) {
          invokeTauri('app_toggle_maximize').catch(() => {});
        }
      });
    }
  }

  // 2. Navigation Tabs
  function setupNavTabs() {
    DOM.navTabs.forEach(tab => {
      tab.addEventListener('click', () => {
        DOM.navTabs.forEach(t => t.classList.remove('active'));
        tab.classList.add('active');
        state.activeTab = tab.getAttribute('data-tab');
        renderActiveMainTab();
      });
    });
  }

  function renderActiveMainTab() {
    const isServers = state.activeTab === 'servers';
    const workspace = document.getElementById('workspace-servers-layout');
    const groupBar = document.getElementById('group-filter-bar');

    if (workspace) workspace.style.display = isServers ? 'flex' : 'none';
    if (groupBar) groupBar.style.display = isServers ? 'flex' : 'none';

    ['scripts', 'cmds', 'systemd', 'sensors'].forEach(v => {
      const el = document.getElementById('view-' + v);
      if (el) el.style.display = (state.activeTab === v) ? 'flex' : 'none';
    });
  }

  // 3. Subtabs Router (Overview vs Individual Server Log View)
  function setupSubtabsRouter() {
    if (!DOM.subtabsContainer) return;
    DOM.subtabsContainer.addEventListener('click', (e) => {
      const tab = e.target.closest('.subtab-item');
      if (tab) {
        const sub = tab.getAttribute('data-subtab');
        switchSubtab(sub);
      }
    });

    document.addEventListener('click', (e) => {
      const target = e.target.closest('[data-open-tab]');
      if (target) {
        const k = target.getAttribute('data-open-tab');
        switchSubtab(k);
      }
    });
  }

  function switchSubtab(subtabKey) {
    state.activeSubtab = subtabKey;
    document.querySelectorAll('.subtab-item').forEach(s => {
      if (s.getAttribute('data-subtab') === subtabKey) {
        s.classList.add('active');
      } else {
        s.classList.remove('active');
      }
    });

    if (subtabKey === 'overview') {
      if (DOM.overviewContainer) DOM.overviewContainer.style.display = 'flex';
      if (DOM.serverTabView) DOM.serverTabView.style.display = 'none';
    } else {
      if (DOM.overviewContainer) DOM.overviewContainer.style.display = 'none';
      if (DOM.serverTabView) DOM.serverTabView.style.display = 'flex';
      renderServerTabView(subtabKey);
    }
  }

  function renderServerTabView(key) {
    const srv = state.servers.find(s => s.key === key);
    if (!srv) return;

    if (DOM.tabSrvTitle) DOM.tabSrvTitle.textContent = srv.name;
    const isRunning = srv.status === 'running';
    if (DOM.tabSrvDot) {
      DOM.tabSrvDot.style.color = isRunning ? '#10b981' : '#64748b';
    }
    if (DOM.tabSrvBadge) {
      DOM.tabSrvBadge.textContent = isRunning ? 'Running' : 'Stopped';
      DOM.tabSrvBadge.className = isRunning ? 'server-view-badge running' : 'server-view-badge';
    }
    if (DOM.tabSrvCwd) {
      DOM.tabSrvCwd.textContent = `cwd: ${srv.cwd}`;
    }
    if (DOM.tabBtnToggle) {
      DOM.tabBtnToggle.className = isRunning ? 'btn-action-stop' : 'btn-action-start';
      DOM.tabBtnToggle.innerHTML = `<span class="icon">${isRunning ? '&#xe167;' : '&#xe13c;'}</span><span>${isRunning ? 'Stop' : 'Start'}</span>`;
      DOM.tabBtnToggle.onclick = () => toggleServer(srv.key);
    }
    if (DOM.tabBtnTerminal) {
      DOM.tabBtnTerminal.onclick = () => invokeTauri('launcher_open_terminal', { cwd: srv.cwd, command: srv.command });
    }
    if (DOM.tabBtnFolder) {
      DOM.tabBtnFolder.onclick = () => invokeTauri('launcher_open_folder', { path: srv.cwd });
    }
    if (DOM.tabBtnCode) {
      DOM.tabBtnCode.onclick = () => invokeTauri('launcher_open_terminal', { cwd: srv.cwd, command: 'code .' });
    }

    // Render custom actions and links
    if (DOM.tabCustomActionsBar) {
      const actions = srv.actions || [];
      const links = srv.links || [];
      if (actions.length > 0 || links.length > 0) {
        DOM.tabCustomActionsBar.style.display = 'flex';
        DOM.tabCustomActionsBar.innerHTML = `
          ${actions.map(a => `
            <button class="btn-pill btn-quick-launch" data-custom-cmd="${escapeHtml(a.command)}" style="font-size: 11px; padding: 4px 10px;">
              <span class="icon">&#xe181;</span>
              <span>${escapeHtml(a.label)}</span>
            </button>
          `).join('')}
          ${links.map(l => `
            <button class="btn-pill btn-quick-launch" data-custom-url="${escapeHtml(l.url)}" style="font-size: 11px; padding: 4px 10px; color: #38bdf8;">
              <span class="icon">&#xe0e8;</span>
              <span>${escapeHtml(l.label)}</span>
            </button>
          `).join('')}
        `;

        DOM.tabCustomActionsBar.querySelectorAll('[data-custom-cmd]').forEach(b => {
          b.onclick = () => {
            const cmd = b.getAttribute('data-custom-cmd');
            invokeTauri('launcher_execute_command', { command: cmd, cwd: srv.cwd });
            showToast(`Executed: ${cmd}`);
          };
        });

        DOM.tabCustomActionsBar.querySelectorAll('[data-custom-url]').forEach(b => {
          b.onclick = () => {
            const url = b.getAttribute('data-custom-url');
            invokeTauri('launcher_open_browser', { url });
          };
        });
      } else {
        DOM.tabCustomActionsBar.style.display = 'none';
      }
    }
  }

  // 4. Server Start / Stop Lifecycle
  function toggleServer(key) {
    const srv = state.servers.find(s => s.key === key);
    if (!srv) return;

    if (srv.status === 'running') {
      srv.status = 'stopped';
      invokeTauri('launcher_stop_server', { key }).catch(() => {});
      showToast(`Stopped ${srv.name}`);
    } else {
      srv.status = 'running';
      invokeTauri('launcher_start_server', { key }).catch(() => {});
      showToast(`Started ${srv.name}`);
    }

    updateServerCardsUi(key);
    updateOverviewStats();
    if (state.activeSubtab === key) {
      renderServerTabView(key);
    }
  }

  function updateServerCardsUi(key) {
    const srv = state.servers.find(s => s.key === key);
    if (!srv) return;
    const isRunning = srv.status === 'running';

    ['card-', 'managed-card-'].forEach(prefix => {
      const card = document.getElementById(prefix + key);
      if (card) {
        const dot = card.querySelector('.status-dot');
        const pill = card.querySelector('.status-pill');
        const btn = card.querySelector('[data-action="toggle-server"]');

        if (dot) dot.className = isRunning ? 'status-dot running' : 'status-dot';
        if (pill) {
          pill.className = isRunning ? 'status-pill running' : 'status-pill';
          pill.textContent = isRunning ? 'Running' : 'Stopped';
        }
        if (btn) {
          btn.className = isRunning ? 'btn-action-stop' : 'btn-action-start';
          btn.innerHTML = `<span class="icon">${isRunning ? '&#xe167;' : '&#xe13c;'}</span><span>${isRunning ? 'Stop' : 'Start'}</span>`;
        }
      }
    });
  }

  function updateOverviewStats() {
    const running = state.servers.filter(s => s.status === 'running').length;
    const total = state.servers.length;
    if (DOM.overviewBadge) DOM.overviewBadge.textContent = `${running}/${total}`;
    if (DOM.metaCount) DOM.metaCount.textContent = total.toString();
  }

  // 5. Global Action Delegation
  function setupActionDelegation() {
    document.addEventListener('click', (e) => {
      const toggleBtn = e.target.closest('[data-action="toggle-server"]');
      if (toggleBtn) {
        const k = toggleBtn.getAttribute('data-key');
        toggleServer(k);
        return;
      }

      const termBtn = e.target.closest('[data-action="terminal"]');
      if (termBtn) {
        const k = termBtn.getAttribute('data-key');
        const s = state.servers.find(srv => srv.key === k);
        if (s) invokeTauri('launcher_open_terminal', { cwd: s.cwd, command: s.command });
        return;
      }

      const folderBtn = e.target.closest('[data-action="folder"]');
      if (folderBtn) {
        const k = folderBtn.getAttribute('data-key');
        const s = state.servers.find(srv => srv.key === k);
        if (s) invokeTauri('launcher_open_folder', { path: s.cwd });
        return;
      }

      const codeBtn = e.target.closest('[data-action="code"]');
      if (codeBtn) {
        const k = codeBtn.getAttribute('data-key');
        const s = state.servers.find(srv => srv.key === k);
        if (s && s.port) {
          invokeTauri('launcher_open_browser', { url: `http://localhost:${s.port}` });
        } else if (s) {
          invokeTauri('launcher_open_terminal', { cwd: s.cwd, command: 'code .' });
        }
        return;
      }

      const delBtn = e.target.closest('[data-action="delete"]');
      if (delBtn) {
        const k = delBtn.getAttribute('data-key');
        if (confirm('Delete this server?')) {
          state.servers = state.servers.filter(s => s.key !== k);
          invokeTauri('launcher_delete_server', { key: k }).catch(() => {});
          const c1 = document.getElementById('card-' + k);
          const c2 = document.getElementById('managed-card-' + k);
          if (c1) c1.remove();
          if (c2) c2.remove();
          updateOverviewStats();
          showToast('Server removed');
        }
        return;
      }
    });

    // Group Filter
    if (DOM.groupPillsContainer) {
      DOM.groupPillsContainer.addEventListener('click', (e) => {
        const pill = e.target.closest('.group-pill');
        if (pill) {
          document.querySelectorAll('.group-pill').forEach(p => p.classList.remove('active'));
          pill.classList.add('active');
          const g = pill.getAttribute('data-group') || 'all';
          state.activeGroup = g;
          filterGroups(g);
        }
      });
    }

    // Modal controls
    if (DOM.btnQuickLaunch) DOM.btnQuickLaunch.onclick = () => openModal(DOM.modalQuickLaunch);
    if (DOM.btnAddServer) DOM.btnAddServer.onclick = () => openModal(DOM.modalAddServer);
    if (DOM.btnSettings) DOM.btnSettings.onclick = () => openModal(DOM.modalSettings);

    const closeModals = () => {
      if (DOM.modalBackdrop) DOM.modalBackdrop.style.display = 'none';
      [DOM.modalQuickLaunch, DOM.modalAddServer, DOM.modalAddGroup, DOM.modalSettings].forEach(m => {
        if (m) m.style.display = 'none';
      });
    };

    document.querySelectorAll('.btn-close-modal, #btn-cancel-add, #btn-cancel-group, #btn-save-settings').forEach(b => {
      b.onclick = closeModals;
    });

    if (DOM.modalBackdrop) {
      DOM.modalBackdrop.onclick = (e) => {
        if (e.target === DOM.modalBackdrop) closeModals();
      };
    }

    // Keyboard Shortcuts
    window.addEventListener('keydown', (e) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        DOM.globalSearch && DOM.globalSearch.focus();
      }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'p') {
        e.preventDefault();
        openModal(DOM.modalQuickLaunch);
        DOM.qlSearchInput && DOM.qlSearchInput.focus();
      }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'f') {
        e.preventDefault();
        DOM.subbarSearch && DOM.subbarSearch.focus();
      }
      if (e.key === 'Escape') {
        closeModals();
      }
    });
  }

  function filterGroups(group) {
    document.querySelectorAll('.group-section').forEach(sec => {
      const g = sec.getAttribute('data-group-section');
      if (group === 'all' || group === g) {
        sec.style.display = 'flex';
      } else {
        sec.style.display = 'none';
      }
    });
  }

  function openModal(el) {
    if (DOM.modalBackdrop) DOM.modalBackdrop.style.display = 'flex';
    if (el) el.style.display = 'flex';
  }

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
      DOM.btnRefresh.onclick = () => {
        state.scanElapsedSec = 0;
        showToast('Scanning network sockets and port listeners...');
        invokeTauri('launcher_scan_listeners').then(res => {
          showToast(`Found ${res.listeners.length} listening sockets`);
        }).catch(() => {});
      };
    }
  }

  function syncBackend() {
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
          updateOverviewStats();
        }
      }
    }).catch(err => {
      console.log('Backend sync notice:', err.message);
    });
  }

  function escapeHtml(str) {
    return String(str || '').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  }

  function init() {
    setupWindowControls();
    setupNavTabs();
    setupSubtabsRouter();
    setupActionDelegation();
    setupScanTimer();
    updateOverviewStats();
    syncBackend();
    console.log('Server Launcher ready.');
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
})();

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
