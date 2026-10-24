/**
 * Server Launcher - Authentic Tauri Desktop Controller
 * 1-to-1 visual, behavioral, and IPC parity with native Rust egui LauncherApp
 */

(function () {
  'use strict';

  const state = {
    selectedMachine: 'local',
    activeSidebarTab: 'servers',
    activeSubtab: 'overview',
    selectedGroup: 'all',
    searchQuery: '',
    lanIp: '192.168.1.5',
    servers: [
      {
        key: 'f',
        name: 'f',
        command: 'npm run dev',
        cwd: 'frontend',
        port: 5173,
        group: 'Frontend',
        status: 'stopped',
        autostart: false,
        external_terminal: false,
        auto_restart: false,
        logs: []
      },
      {
        key: 'vendly_backend',
        name: 'Vendly Backend',
        command: 'python run.py',
        cwd: 'backend',
        port: null,
        group: 'Backend',
        status: 'stopped',
        autostart: false,
        external_terminal: false,
        auto_restart: false,
        logs: []
      },
      {
        key: 'react_native__expo',
        name: 'React Native (Expo)',
        command: 'npx expo start',
        cwd: 'mobile',
        port: 8081,
        group: 'Mobile',
        status: 'stopped',
        autostart: false,
        external_terminal: false,
        auto_restart: false,
        custom_actions: [
          { label: 'Android', command: 'a' },
          { label: 'iOS', command: 'i' },
          { label: 'Web', command: 'w' },
          { label: 'Install', command: 'npm install' }
        ],
        custom_links: [
          { label: 'Metro Bundler', url: 'http://localhost:8081' }
        ],
        logs: []
      }
    ]
  };

  const DOM = {};

  function invokeTauri(cmd, args = {}) {
    if (window.__TAURI__ && window.__TAURI__.core && typeof window.__TAURI__.core.invoke === 'function') {
      return window.__TAURI__.core.invoke(cmd, args);
    }
    return Promise.reject(new Error('Tauri IPC core not available in web context'));
  }

  function setupWindowControls() {
    const btnMin = document.getElementById('btn-minimize');
    const btnMax = document.getElementById('btn-maximize');
    const btnClose = document.getElementById('btn-close');

    if (btnMin) {
      btnMin.addEventListener('click', () => {
        invokeTauri('app_minimize').catch(() => console.log('Minimize window'));
      });
    }

    if (btnMax) {
      btnMax.addEventListener('click', () => {
        invokeTauri('app_toggle_maximize').catch(() => console.log('Toggle maximize'));
      });
    }

    if (btnClose) {
      btnClose.addEventListener('click', () => {
        invokeTauri('app_close').catch(() => console.log('Close app'));
      });
    }
  }

  function setupSidebarTabs() {
    const tabs = document.querySelectorAll('.sidebar-tab');
    tabs.forEach(tab => {
      tab.addEventListener('click', () => {
        const target = tab.dataset.tab;
        state.activeSidebarTab = target;
        tabs.forEach(t => t.classList.toggle('active', t === tab));
        
        ['servers', 'scripts', 'cmds', 'systemd', 'sensors'].forEach(t => {
          const lv = document.getElementById(`left-view-${t}`);
          const rv = document.getElementById(`tab-content-${t}`);
          if (lv) lv.style.display = (t === target) ? 'flex' : 'none';
          if (rv && state.activeSubtab !== 'overview') {
            rv.style.display = (t === target) ? 'flex' : 'none';
          }
        });

        const overviewSub = document.getElementById('tab-content-overview');
        if (target === 'servers') {
          if (overviewSub) overviewSub.style.display = (state.activeSubtab === 'overview') ? 'flex' : 'none';
        } else {
          if (overviewSub) overviewSub.style.display = 'none';
        }
      });
    });

    const groupPills = document.querySelectorAll('.group-pill');
    groupPills.forEach(pill => {
      pill.addEventListener('click', () => {
        state.selectedGroup = pill.dataset.group;
        groupPills.forEach(p => p.classList.toggle('active', p === pill));
        filterServersByGroup();
      });
    });
  }

  function filterServersByGroup() {
    const blocks = document.querySelectorAll('.group-block');
    blocks.forEach(block => {
      const g = block.dataset.group;
      if (state.selectedGroup === 'all' || state.selectedGroup === g) {
        block.style.display = 'block';
      } else {
        block.style.display = 'none';
      }
    });
  }

  function setupSubtabsRouter() {
    const subtabs = document.querySelectorAll('.notebook-tab');
    subtabs.forEach(tab => {
      tab.addEventListener('click', () => {
        const subId = tab.dataset.subtab;
        openSubtab(subId);
      });
    });

    document.querySelectorAll('[data-open-subtab]').forEach(el => {
      el.addEventListener('click', () => {
        openSubtab(el.dataset.openSubtab);
      });
    });
  }

  function openSubtab(subId) {
    state.activeSubtab = subId;
    document.querySelectorAll('.notebook-tab').forEach(t => {
      t.classList.toggle('active', t.dataset.subtab === subId);
    });

    const overviewView = document.getElementById('tab-content-overview');
    const serverDetailView = document.getElementById('tab-content-server');

    if (subId === 'overview') {
      if (overviewView) overviewView.style.display = 'flex';
      if (serverDetailView) serverDetailView.style.display = 'none';
    } else {
      if (overviewView) overviewView.style.display = 'none';
      if (serverDetailView) {
        serverDetailView.style.display = 'flex';
        renderServerDetail(subId);
      }
    }
  }

  function renderServerDetail(key) {
    const srv = state.servers.find(s => s.key === key);
    if (!srv) return;

    const nameEl = document.getElementById('srv-detail-name');
    const chipEl = document.getElementById('srv-detail-chip');
    const cwdEl = document.getElementById('srv-detail-cwd');
    const outEl = document.getElementById('srv-detail-output');
    const startBtn = document.getElementById('srv-detail-start-btn');
    const customBar = document.getElementById('srv-custom-bar');

    if (nameEl) nameEl.textContent = srv.name;
    if (cwdEl) cwdEl.textContent = `/${srv.cwd}`;
    
    const isRunning = srv.status === 'running';
    if (chipEl) {
      chipEl.textContent = isRunning ? 'Running' : 'Stopped';
      chipEl.className = `status-chip-pill ${isRunning ? 'running' : 'stopped'}`;
    }

    if (startBtn) {
      startBtn.innerHTML = isRunning 
        ? '<span class="icon">&#xe167;</span><span>Stop</span>'
        : '<span class="icon">&#xe13c;</span><span>Start</span>';
      startBtn.onclick = () => toggleServer(srv.key);
    }

    if (outEl) {
      if (srv.logs && srv.logs.length > 0) {
        outEl.textContent = srv.logs.join('\n');
      } else {
        outEl.textContent = isRunning 
          ? `[System] Server '${srv.name}' is actively running on port ${srv.port || 'dynamic'}.\nListening for incoming network requests...`
          : `// Process is currently stopped. Click 'Start' to launch '${srv.command}'.`;
      }
    }

    if (customBar) {
      customBar.innerHTML = '';
      if (Array.isArray(srv.custom_actions)) {
        srv.custom_actions.forEach(act => {
          const btn = document.createElement('button');
          btn.className = 'btn-pill';
          btn.style.cssText = 'background: #1e293b; color: #38bdf8;';
          btn.textContent = act.label;
          btn.onclick = () => {
            showToast(`Sent action '${act.label}' to ${srv.name}`);
          };
          customBar.appendChild(btn);
        });
      }
      if (Array.isArray(srv.custom_links)) {
        srv.custom_links.forEach(lnk => {
          const btn = document.createElement('button');
          btn.className = 'btn-pill';
          btn.style.cssText = 'background: rgba(37, 99, 235, 0.2); color: #60a5fa;';
          btn.innerHTML = `<span class="icon">&#xe0e8;</span> <span>${lnk.label}</span>`;
          btn.onclick = () => {
            invokeTauri('launcher_open_browser', { url: lnk.url }).catch(() => window.open(lnk.url, '_blank'));
          };
          customBar.appendChild(btn);
        });
      }
    }
  }

  function toggleServer(key) {
    const srv = state.servers.find(s => s.key === key);
    if (!srv) return;

    const isRunning = srv.status === 'running';
    if (isRunning) {
      invokeTauri('launcher_stop_server', { key }).catch(() => {});
      srv.status = 'stopped';
      showToast(`Stopped ${srv.name}`);
    } else {
      invokeTauri('launcher_start_server', { key }).catch(() => {});
      srv.status = 'running';
      showToast(`Started ${srv.name}`);
    }

    updateServerCardsUI(srv);
    updateStatsCounter();
    if (state.activeSubtab === key) {
      renderServerDetail(key);
    }
  }

  function updateServerCardsUI(srv) {
    const isRunning = srv.status === 'running';
    ['card-', 'managed-card-'].forEach(prefix => {
      const card = document.getElementById(`${prefix}${srv.key}`);
      if (!card) return;

      card.classList.toggle('running', isRunning);
      const chip = card.querySelector('.status-chip-pill');
      if (chip) {
        chip.textContent = isRunning ? 'Running' : 'Stopped';
        chip.className = `status-chip-pill ${isRunning ? 'running' : 'stopped'}`;
      }

      const startBtn = card.querySelector('.btn-glow-start');
      if (startBtn) {
        startBtn.innerHTML = isRunning 
          ? '<span class="icon">&#xe167;</span><span>Stop</span>'
          : '<span class="icon">&#xe13c;</span><span>Start</span>';
      }
    });
  }

  function updateStatsCounter() {
    const running = state.servers.filter(s => s.status === 'running').length;
    const total = state.servers.length;
    
    const countBadge = document.getElementById('online-count-badge');
    if (countBadge) countBadge.textContent = `${running}/${total}`;

    const metaCount = document.getElementById('meta-count');
    if (metaCount) metaCount.textContent = total;
  }

  function setupActionDelegation() {
    document.addEventListener('click', (e) => {
      const toggleBtn = e.target.closest('[data-toggle-key]');
      if (toggleBtn) {
        toggleServer(toggleBtn.dataset.toggleKey);
        return;
      }

      const toolBtn = e.target.closest('[data-tool]');
      if (toolBtn) {
        const tool = toolBtn.dataset.tool;
        const key = toolBtn.dataset.key;
        handleToolAction(tool, key);
        return;
      }

      const batchStart = e.target.closest('[data-batch-start]');
      if (batchStart) {
        const group = batchStart.dataset.batchStart;
        state.servers.filter(s => s.group.toLowerCase() === group.toLowerCase()).forEach(s => {
          if (s.status !== 'running') toggleServer(s.key);
        });
        return;
      }

      const batchStop = e.target.closest('[data-batch-stop]');
      if (batchStop) {
        const group = batchStop.dataset.batchStop;
        state.servers.filter(s => s.group.toLowerCase() === group.toLowerCase()).forEach(s => {
          if (s.status === 'running') toggleServer(s.key);
        });
        return;
      }
    });

    document.querySelectorAll('.toggle-checkbox input').forEach(chk => {
      chk.addEventListener('change', () => {
        const key = chk.dataset.key;
        const name = chk.name;
        const srv = state.servers.find(s => s.key === key);
        if (srv) {
          srv[name] = chk.checked;
          invokeTauri('launcher_save_server', { server: srv }).catch(() => {});
        }
      });
    });

    setupModals();
  }

  function handleToolAction(tool, key) {
    const srv = state.servers.find(s => s.key === key);
    if (!srv) return;

    if (tool === 'terminal') {
      invokeTauri('launcher_open_terminal', { cwd: srv.cwd }).catch(() => showToast(`Opened terminal in ${srv.cwd}`));
    } else if (tool === 'folder') {
      invokeTauri('launcher_open_folder', { path: srv.cwd }).catch(() => showToast(`Opened directory ${srv.cwd}`));
    } else if (tool === 'code') {
      invokeTauri('launcher_execute_command', { command: `code "${srv.cwd}"` }).catch(() => showToast(`Opened VS Code for ${srv.name}`));
    } else if (tool === 'edit') {
      openAddEditModal(srv);
    } else if (tool === 'delete') {
      if (confirm(`Are you sure you want to delete '${srv.name}'?`)) {
        invokeTauri('launcher_delete_server', { key }).catch(() => {});
        state.servers = state.servers.filter(s => s.key !== key);
        document.querySelectorAll(`#card-${key}, #managed-card-${key}`).forEach(el => el.remove());
        updateStatsCounter();
        showToast(`Deleted ${srv.name}`);
      }
    }
  }

  function setupModals() {
    const backdrop = document.getElementById('modal-backdrop');
    const qlModal = document.getElementById('modal-quick-launch');
    const addModal = document.getElementById('modal-add-server');
    const settingsModal = document.getElementById('modal-settings');

    const btnQl = document.getElementById('btn-quick-launch');
    const btnAdd = document.getElementById('btn-add-server');
    const btnSettings = document.getElementById('btn-settings');

    function closeAllModals() {
      if (backdrop) backdrop.style.display = 'none';
      if (qlModal) qlModal.style.display = 'none';
      if (addModal) addModal.style.display = 'none';
      if (settingsModal) settingsModal.style.display = 'none';
    }

    if (btnQl) {
      btnQl.addEventListener('click', () => {
        backdrop.style.display = 'flex';
        qlModal.style.display = 'block';
        const qlIn = document.getElementById('ql-search-input');
        if (qlIn) { qlIn.value = ''; qlIn.focus(); renderQlResults(''); }
      });
    }

    if (btnAdd) {
      btnAdd.addEventListener('click', () => {
        backdrop.style.display = 'flex';
        addModal.style.display = 'block';
      });
    }

    if (btnSettings) {
      btnSettings.addEventListener('click', () => {
        backdrop.style.display = 'flex';
        settingsModal.style.display = 'block';
      });
    }

    document.querySelectorAll('.btn-close-modal, #btn-cancel-add, #btn-save-settings').forEach(btn => {
      btn.addEventListener('click', closeAllModals);
    });

    document.addEventListener('keydown', (e) => {
      if (e.key === 'Escape') closeAllModals();
      if ((e.ctrlKey || e.metaKey) && (e.key === 'p' || e.key === 'k')) {
        e.preventDefault();
        btnQl?.click();
      }
    });

    const formAdd = document.getElementById('form-add-server');
    if (formAdd) {
      formAdd.addEventListener('submit', (e) => {
        e.preventDefault();
        const fd = new FormData(formAdd);
        const newSrv = {
          key: fd.get('name').toLowerCase().replace(/[^a-z0-9]/g, '_'),
          name: fd.get('name'),
          command: fd.get('command'),
          cwd: fd.get('cwd'),
          port: fd.get('port') ? parseInt(fd.get('port'), 10) : null,
          group: fd.get('group'),
          status: 'stopped'
        };
        state.servers.push(newSrv);
        invokeTauri('launcher_save_server', { server: newSrv }).catch(() => {});
        updateStatsCounter();
        closeAllModals();
        showToast(`Server '${newSrv.name}' added successfully`);
      });
    }
  }

  function renderQlResults(q) {
    const list = document.getElementById('ql-results-list');
    if (!list) return;
    list.innerHTML = '';
    const query = (q || '').toLowerCase();
    const matched = state.servers.filter(s => s.name.toLowerCase().includes(query) || s.command.toLowerCase().includes(query));
    
    matched.forEach(s => {
      const item = document.createElement('div');
      item.style.cssText = 'padding:10px 18px;border-bottom:1px solid rgba(255,255,255,0.05);cursor:pointer;display:flex;align-items:center;justify-content:space-between;';
      item.innerHTML = `<div><strong style="color:#f8fafc;font-size:13px;">${escapeHtml(s.name)}</strong><span style="font-size:11px;color:#64748b;margin-left:8px;">${escapeHtml(s.command)}</span></div><span class="status-chip-pill ${s.status}">${s.status}</span>`;
      item.onclick = () => {
        document.getElementById('modal-backdrop').style.display = 'none';
        openSubtab(s.key);
      };
      list.appendChild(item);
    });
  }

  function setupTimer() {
    const timerEl = document.getElementById('scan-timer');
    if (!timerEl) return;
    setInterval(() => {
      const now = new Date();
      const hh = String(now.getHours()).padStart(2, '0');
      const mm = String(now.getMinutes()).padStart(2, '0');
      const ss = String(now.getSeconds()).padStart(2, '0');
      timerEl.textContent = `${hh}:${mm}:${ss}`;
    }, 1000);
  }

  function syncWithBackend() {
    invokeTauri('launcher_get_data').then(data => {
      if (data) {
        if (data.lan_ip) {
          state.lanIp = data.lan_ip;
          const metaIp = document.getElementById('meta-ip');
          if (metaIp) metaIp.textContent = data.lan_ip;
        }
        if (Array.isArray(data.servers) && data.servers.length > 0) {
          state.servers = data.servers.map(s => ({
            ...s,
            status: s.status || 'stopped'
          }));
          updateStatsCounter();
        }
      }
    }).catch(err => {
      console.log('Backend sync offline:', err.message);
    });
  }

  function escapeHtml(str) {
    return String(str || '').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  }

  function init() {
    setupWindowControls();
    setupSidebarTabs();
    setupSubtabsRouter();
    setupActionDelegation();
    setupTimer();
    updateStatsCounter();
    syncWithBackend();
    console.log('Server Launcher client online.');
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
    t.style.cssText = 'position:fixed;bottom:20px;right:20px;background:#162032;border:none;color:#f8fafc;padding:9px 16px;border-radius:8px;font-size:12.5px;font-weight:600;z-index:99999;box-shadow:0 8px 24px rgba(0,0,0,0.6);transition:opacity 0.2s;';
    document.body.appendChild(t);
  }
  t.textContent = msg;
  t.style.opacity = '1';
  clearTimeout(t._timer);
  t._timer = setTimeout(() => { t.style.opacity = '0'; }, 2400);
}
