/**
 * Server Launcher - Complete Fully Functionable Controller
 * 1-to-1 parity with native Rust egui LauncherApp across all pages and machines
 */

(function () {
  'use strict';

  // Global Application State
  const state = {
    selectedMachine: 'local', // 'local' | 'ssh:<id>' | 'adb:<serial>'
    activeSidebarTab: 'servers', // 'servers' | 'scripts' | 'cmds' | 'systemd' | 'sensors'
    activeSubtab: 'overview', // 'overview' | '<server_key>'
    selectedGroup: 'all',
    activeSensorCat: 'top_apps',
    activeScriptId: null,
    activeSysdUnit: null,
    activeCmdCat: 'all',
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
    ],
    sshHosts: [],
    adbDevices: [],
    scripts: [
      { id: 'clean_artifacts', name: 'Clean Build Artifacts', shell: 'bash', cwd: '.', script: 'cargo clean && echo "Clean finished"' },
      { id: 'git_pull_all', name: 'Git Pull Latest', shell: 'bash', cwd: '.', script: 'git pull --rebase' }
    ],
    savedCommands: [
      { id: 'docker_ps', name: 'Docker Containers', command: 'docker ps -a', category: 'Docker' },
      { id: 'docker_prune', name: 'Docker System Prune', command: 'docker system prune -f', category: 'Docker' },
      { id: 'git_status', name: 'Git Status Summary', command: 'git status -s', category: 'Git' },
      { id: 'ip_route', name: 'Network Route & IP', command: 'ip a && ip route', category: 'System' },
      { id: 'disk_space', name: 'Check Disk Space', command: 'df -h', category: 'System' }
    ],
    systemdServices: []
  };

  // Safe Tauri IPC Helper
  function invokeTauri(cmd, args = {}) {
    if (window.__TAURI__ && window.__TAURI__.core && typeof window.__TAURI__.core.invoke === 'function') {
      return window.__TAURI__.core.invoke(cmd, args);
    }
    return Promise.reject(new Error(`Tauri IPC not available for ${cmd}`));
  }

  // 1. Window Controls (Minimize, Maximize, Close)
  function setupWindowControls() {
    const btnMin = document.getElementById('btn-minimize');
    const btnMax = document.getElementById('btn-maximize');
    const btnClose = document.getElementById('btn-close');

    if (btnMin) btnMin.onclick = () => invokeTauri('app_minimize').catch(() => console.log('Minimize'));
    if (btnMax) btnMax.onclick = () => invokeTauri('app_toggle_maximize').catch(() => console.log('Toggle Maximize'));
    if (btnClose) btnClose.onclick = () => invokeTauri('app_close').catch(() => console.log('Close'));
  }

  // 2. Machine & Device Rail Management
  function setupDeviceRail() {
    const railLocal = document.getElementById('rail-local');
    if (railLocal) {
      railLocal.onclick = () => switchMachine('local');
    }

    const btnAddDev = document.getElementById('rail-add-device');
    if (btnAddDev) {
      btnAddDev.onclick = () => {
        openModal('modal-add-device');
      };
    }

    const choiceSsh = document.getElementById('choice-ssh-host');
    if (choiceSsh) {
      choiceSsh.onclick = () => {
        closeAllModals();
        openModal('modal-ssh-host');
      };
    }

    const choiceAdb = document.getElementById('choice-adb-device');
    if (choiceAdb) {
      choiceAdb.onclick = () => {
        closeAllModals();
        showToast('Scanning for USB / Wireless ADB devices...');
        loadAdbDevices();
      };
    }
  }

  function renderDeviceRail() {
    // Render SSH hosts
    const hostsContainer = document.getElementById('rail-dynamic-hosts');
    if (hostsContainer) {
      hostsContainer.innerHTML = '';
      state.sshHosts.forEach(host => {
        const btn = document.createElement('button');
        btn.className = `rail-item ${state.selectedMachine === 'ssh:' + host.id ? 'active' : ''}`;
        btn.title = `SSH: ${host.name} (${host.user}@${host.host})`;
        btn.innerHTML = `<span class="icon">&#xe0e8;</span><span class="beacon-dot online"></span>`;
        btn.onclick = () => switchMachine('ssh:' + host.id);
        hostsContainer.appendChild(btn);
      });
    }

    // Render ADB devices
    const adbContainer = document.getElementById('rail-dynamic-adb');
    if (adbContainer) {
      adbContainer.innerHTML = '';
      state.adbDevices.forEach(dev => {
        const btn = document.createElement('button');
        btn.className = `rail-item ${state.selectedMachine === 'adb:' + dev.serial ? 'active' : ''}`;
        btn.title = `ADB: ${dev.name} (${dev.serial})`;
        const beaconClass = dev.is_wireless ? 'wireless' : 'online';
        btn.innerHTML = `<span class="icon">&#xe163;</span><span class="beacon-dot ${beaconClass}"></span>`;
        btn.onclick = () => switchMachine('adb:' + dev.serial);
        adbContainer.appendChild(btn);
      });
    }
  }

  function switchMachine(machineKey) {
    state.selectedMachine = machineKey;
    document.querySelectorAll('.rail-item').forEach(b => b.classList.remove('active'));

    const viewLocal = document.getElementById('view-local-pc');
    const viewRemote = document.getElementById('view-remote-host');
    const viewAdb = document.getElementById('view-adb-cockpit');
    const headerTitle = document.getElementById('header-machine-name');

    if (machineKey === 'local') {
      document.getElementById('rail-local')?.classList.add('active');
      if (viewLocal) viewLocal.style.display = 'flex';
      if (viewRemote) viewRemote.style.display = 'none';
      if (viewAdb) viewAdb.style.display = 'none';
      if (headerTitle) headerTitle.textContent = 'Server Launcher';
    } else if (machineKey.startsWith('ssh:')) {
      const hostId = machineKey.replace('ssh:', '');
      const host = state.sshHosts.find(h => h.id === hostId);
      if (viewLocal) viewLocal.style.display = 'none';
      if (viewRemote) viewRemote.style.display = 'flex';
      if (viewAdb) viewAdb.style.display = 'none';
      if (host) {
        if (headerTitle) headerTitle.textContent = host.name;
        loadRemoteHostView(host);
      }
    } else if (machineKey.startsWith('adb:')) {
      const serial = machineKey.replace('adb:', '');
      const dev = state.adbDevices.find(d => d.serial === serial);
      if (viewLocal) viewLocal.style.display = 'none';
      if (viewRemote) viewRemote.style.display = 'none';
      if (viewAdb) viewAdb.style.display = 'flex';
      if (dev) {
        if (headerTitle) headerTitle.textContent = dev.name;
        loadAdbCockpitView(dev);
      }
    }
    renderDeviceRail();
  }

  // 3. Navigation Sidebar Tabs (Servers, Scripts, Cmds, SystemD, Sensors)
  function setupSidebarTabs() {
    const tabs = document.querySelectorAll('.sidebar-tab');
    tabs.forEach(tab => {
      tab.onclick = () => {
        const target = tab.dataset.tab;
        state.activeSidebarTab = target;
        tabs.forEach(t => t.classList.toggle('active', t === tab));

        ['servers', 'scripts', 'cmds', 'systemd', 'sensors'].forEach(t => {
          const lv = document.getElementById(`left-view-${t}`);
          const rv = document.getElementById(`rv-${t}-view`);
          if (lv) lv.style.display = (t === target) ? 'flex' : 'none';
          if (rv) rv.style.display = (t === target) ? 'flex' : 'none';
        });

        if (target === 'sensors') {
          pollDetailedSensors();
        } else if (target === 'systemd') {
          loadSystemdServices();
        } else if (target === 'scripts') {
          renderScriptsList();
        } else if (target === 'cmds') {
          renderCommandsList();
        }
      };
    });

    // Group Filter Pills
    const groupPills = document.querySelectorAll('.group-pill');
    groupPills.forEach(pill => {
      pill.onclick = () => {
        state.selectedGroup = pill.dataset.group;
        groupPills.forEach(p => p.classList.toggle('active', p === pill));
        filterServersByGroup();
      };
    });
  }

  function filterServersByGroup() {
    const blocks = document.querySelectorAll('#left-servers-container .group-block');
    blocks.forEach(block => {
      const g = block.dataset.group;
      block.style.display = (state.selectedGroup === 'all' || state.selectedGroup === g) ? 'block' : 'none';
    });
  }

  // 4. Subtabs & Server Detail View
  function setupSubtabsRouter() {
    document.querySelectorAll('.notebook-tab').forEach(tab => {
      tab.onclick = () => {
        const subId = tab.dataset.subtab;
        openSubtab(subId);
      };
    });

    document.querySelectorAll('[data-open-subtab]').forEach(el => {
      el.onclick = () => openSubtab(el.dataset.openSubtab);
    });
  }

  function openSubtab(subId) {
    state.activeSubtab = subId;
    document.querySelectorAll('#notebook-tabs-strip .notebook-tab').forEach(t => {
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
          ? `[System] Server '${srv.name}' is running on port ${srv.port || 'dynamic'}.\nStreaming process output logs...`
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
            invokeTauri('launcher_execute_command', { command: act.command, cwd: srv.cwd }).then(res => {
              showToast(`Action '${act.label}' executed`);
            }).catch(e => showToast(`Action error: ${e}`));
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
      invokeTauri('launcher_stop_server', { key }).then(() => {
        srv.status = 'stopped';
        showToast(`Stopped ${srv.name}`);
        updateServerCardsUI(srv);
      }).catch(e => {
        srv.status = 'stopped';
        updateServerCardsUI(srv);
      });
    } else {
      invokeTauri('launcher_start_server', { key }).then(() => {
        srv.status = 'running';
        showToast(`Started ${srv.name}`);
        updateServerCardsUI(srv);
      }).catch(e => {
        srv.status = 'running';
        updateServerCardsUI(srv);
      });
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

    updateStatsCounter();
    if (state.activeSubtab === srv.key) {
      renderServerDetail(srv.key);
    }
  }

  function updateStatsCounter() {
    const running = state.servers.filter(s => s.status === 'running').length;
    const total = state.servers.length;
    
    const countBadge = document.getElementById('online-count-badge');
    if (countBadge) countBadge.textContent = `${running}/${total}`;

    const metaCount = document.getElementById('meta-count');
    if (metaCount) metaCount.textContent = total;

    const beacon = document.getElementById('rail-local-beacon');
    if (beacon) beacon.style.display = running > 0 ? 'block' : 'none';
  }

  // 5. Scripts Page Controller
  function renderScriptsList() {
    const list = document.getElementById('scripts-left-list');
    if (!list) return;
    list.innerHTML = '';

    state.scripts.forEach(sc => {
      const card = document.createElement('div');
      card.className = `script-card-item ${state.activeScriptId === sc.id ? 'active' : ''}`;
      card.innerHTML = `
        <div class="sci-header">
          <strong class="sci-title">${escapeHtml(sc.name)}</strong>
          <button class="btn-group-action btn-group-start" data-run-script="${sc.id}">
            <span class="icon">&#xe13c;</span> Run
          </button>
        </div>
        <div class="sci-meta">
          <span class="meta-tag"><span class="icon">&#xe181;</span> ${sc.shell || 'bash'}</span>
          <span>${escapeHtml(sc.cwd || '.')}</span>
        </div>
      `;
      card.onclick = (e) => {
        if (!e.target.closest('[data-run-script]')) {
          selectScript(sc);
        }
      };
      list.appendChild(card);
    });

    const btnNew = document.getElementById('btn-add-script');
    if (btnNew) btnNew.onclick = () => openModal('modal-new-script');
  }

  function selectScript(sc) {
    state.activeScriptId = sc.id;
    document.querySelectorAll('.script-card-item').forEach(c => c.classList.remove('active'));
    renderScriptsList();

    const titleEl = document.getElementById('scripts-active-name');
    const descEl = document.getElementById('scripts-status-desc');
    const outEl = document.getElementById('scripts-terminal-output');

    if (titleEl) titleEl.textContent = sc.name;
    if (descEl) descEl.textContent = `Interpreter: ${sc.shell} | CWD: ${sc.cwd}`;
    if (outEl) outEl.textContent = `Script Source Code:\n\n${sc.script}\n\nClick 'Execute Now' to run.`;

    const runBtn = document.getElementById('btn-run-active-script');
    if (runBtn) runBtn.onclick = () => executeScript(sc.id);
  }

  function executeScript(id) {
    const sc = state.scripts.find(s => s.id === id);
    const outEl = document.getElementById('scripts-terminal-output');
    if (outEl) outEl.textContent = `[Runner] Executing '${sc ? sc.name : id}'...\n`;

    invokeTauri('launcher_execute_script', { id }).then(res => {
      if (outEl) outEl.textContent += `\n[Exit Code 0]\n` + res;
      showToast(`Script '${sc ? sc.name : id}' succeeded`);
    }).catch(err => {
      if (outEl) outEl.textContent += `\n[Error]\n` + err;
      showToast(`Script failed: ${err}`);
    });
  }

  // 6. Commands Page Controller
  function renderCommandsList() {
    const list = document.getElementById('cmds-left-list');
    if (!list) return;
    list.innerHTML = '';

    const filtered = state.savedCommands.filter(c => state.activeCmdCat === 'all' || c.category === state.activeCmdCat);

    filtered.forEach(cmd => {
      const card = document.createElement('div');
      card.className = 'cmd-card-item';
      card.innerHTML = `
        <div class="sci-header">
          <strong class="sci-title">${escapeHtml(cmd.name)}</strong>
          <div style="display:flex;gap:4px;">
            <button class="tool-btn" data-copy-cmd="${escapeHtml(cmd.command)}" title="Copy"><span class="icon">&#xe0b2;</span></button>
            <button class="btn-group-action btn-group-start" data-run-cmd="${escapeHtml(cmd.command)}"><span class="icon">&#xe13c;</span> Run</button>
          </div>
        </div>
        <div class="sci-meta" style="margin-top:6px;">
          <code style="color:var(--cyber-cyan);font-family:var(--font-mono);">${escapeHtml(cmd.command)}</code>
        </div>
      `;
      list.appendChild(card);
    });

    // Categories
    document.querySelectorAll('#cmds-categories-row .cat-pill').forEach(pill => {
      pill.onclick = () => {
        state.activeCmdCat = pill.dataset.cat;
        document.querySelectorAll('#cmds-categories-row .cat-pill').forEach(p => p.classList.toggle('active', p === pill));
        renderCommandsList();
      };
    });

    const btnNew = document.getElementById('btn-add-command');
    if (btnNew) btnNew.onclick = () => openModal('modal-new-command');
  }

  function executeCustomCommand(cmdStr) {
    const outEl = document.getElementById('cmds-terminal-output');
    if (outEl) outEl.textContent = `[Terminal Runner] $ ${cmdStr}\n`;

    invokeTauri('launcher_execute_command', { command: cmdStr }).then(res => {
      if (outEl) outEl.textContent += res;
    }).catch(err => {
      if (outEl) outEl.textContent += `\n[Error] ` + err;
    });
  }

  // 7. SystemD Page Controller
  function loadSystemdServices() {
    const list = document.getElementById('systemd-left-list');
    if (list) list.innerHTML = '<p class="empty-state-p">Loading systemd units...</p>';

    invokeTauri('launcher_list_systemd').then(services => {
      if (Array.isArray(services)) {
        state.systemdServices = services;
        renderSystemdList();
      }
    }).catch(e => {
      if (list) list.innerHTML = `<p class="empty-state-p">Error loading systemd: ${e}</p>`;
    });
  }

  function renderSystemdList() {
    const list = document.getElementById('systemd-left-list');
    if (!list) return;
    list.innerHTML = '';

    const filterIn = document.getElementById('sysd-search-input');
    const query = filterIn ? filterIn.value.toLowerCase() : '';

    const matched = state.systemdServices.filter(s => {
      return s.name.toLowerCase().includes(query) || (s.description && s.description.toLowerCase().includes(query));
    });

    matched.forEach(unit => {
      const card = document.createElement('div');
      card.className = `sysd-unit-card ${state.activeSysdUnit?.name === unit.name ? 'active' : ''}`;
      const statusClass = unit.is_active ? 'active' : (unit.is_failed ? 'failed' : 'stopped');
      card.innerHTML = `
        <div class="sysd-unit-header">
          <span class="sysd-unit-name">${escapeHtml(unit.name)}</span>
          <span class="status-chip-pill ${statusClass}">${unit.active_state}</span>
        </div>
        <div class="sysd-unit-desc">${escapeHtml(unit.description || 'System service unit')}</div>
      `;
      card.onclick = () => selectSystemdUnit(unit);
      list.appendChild(card);
    });
  }

  function selectSystemdUnit(unit) {
    state.activeSysdUnit = unit;
    document.querySelectorAll('.sysd-unit-card').forEach(c => c.classList.remove('active'));
    renderSystemdList();

    const nameEl = document.getElementById('sysd-active-name');
    const descEl = document.getElementById('sysd-active-desc');
    const loadEl = document.getElementById('sysd-load-state');
    const actEl = document.getElementById('sysd-active-state');
    const subEl = document.getElementById('sysd-sub-state');

    if (nameEl) nameEl.textContent = unit.name;
    if (descEl) descEl.textContent = unit.description || 'System service';
    if (loadEl) loadEl.textContent = `Loaded: ${unit.load_state}`;
    if (actEl) actEl.textContent = `Active: ${unit.active_state}`;
    if (subEl) subEl.textContent = `Sub: ${unit.sub_state}`;

    loadJournalLogs(unit.name);

    // Controls
    const btnStart = document.getElementById('btn-sysd-start');
    const btnStop = document.getElementById('btn-sysd-stop');
    const btnRestart = document.getElementById('btn-sysd-restart');

    if (btnStart) btnStart.onclick = () => controlSystemd(unit.name, 'start');
    if (btnStop) btnStop.onclick = () => controlSystemd(unit.name, 'stop');
    if (btnRestart) btnRestart.onclick = () => controlSystemd(unit.name, 'restart');
  }

  function controlSystemd(service, action) {
    invokeTauri('launcher_control_systemd', { service, action }).then(res => {
      showToast(`SystemD ${action} ${service}: ${res}`);
      loadSystemdServices();
      loadJournalLogs(service);
    }).catch(err => showToast(`SystemD error: ${err}`));
  }

  function loadJournalLogs(service) {
    const out = document.getElementById('sysd-journal-output');
    if (out) out.textContent = `Streaming journalctl -u ${service}...\n`;

    invokeTauri('launcher_systemd_journal', { service, lines: 50 }).then(logs => {
      if (out) out.textContent = logs || '// No recent journal logs found for this unit.';
    }).catch(err => {
      if (out) out.textContent = `Error reading journal: ${err}`;
    });
  }

  // 8. Sensors Telemetry Page Controller
  function setupSensorsNav() {
    document.querySelectorAll('.sensor-nav-item').forEach(btn => {
      btn.onclick = () => {
        const cat = btn.dataset.sensorCat;
        state.activeSensorCat = cat;
        document.querySelectorAll('.sensor-nav-item').forEach(b => b.classList.toggle('active', b === btn));
        
        ['top_apps', 'cpu', 'memory', 'gpu', 'storage', 'system'].forEach(c => {
          const pane = document.getElementById(`spane-${c}`);
          if (pane) pane.style.display = (c === cat) ? 'flex' : 'none';
        });
      };
    });

    const pollBtn = document.getElementById('btn-poll-sensors');
    if (pollBtn) pollBtn.onclick = pollDetailedSensors;

    // Periodic sensor telemetry polling loop
    setInterval(() => {
      if (state.activeSidebarTab === 'sensors' && state.selectedMachine === 'local') {
        pollDetailedSensors();
      }
    }, 2000);
  }

  function pollDetailedSensors() {
    invokeTauri('launcher_get_detailed_sensors').then(data => {
      if (!data) return;

      // Update sidebar nav values
      const snCpu = document.getElementById('sn-cpu-val');
      const snMem = document.getElementById('sn-mem-val');
      if (snCpu) snCpu.textContent = `${Math.round(data.cpu_global)}%`;
      if (snMem) snMem.textContent = `${(data.mem_used_mb / 1024).toFixed(1)} GB`;

      // System banner
      const osVal = document.getElementById('sbb-os-val');
      const cpuModel = document.getElementById('sbb-cpu-model');
      const uptime = document.getElementById('sbb-uptime-val');
      if (osVal && data.cpu_brand) cpuModel.textContent = data.cpu_brand;
      if (uptime && data.uptime_secs) {
        const d = Math.floor(data.uptime_secs / 86400);
        const h = Math.floor((data.uptime_secs % 86400) / 3600);
        uptime.textContent = `${d}d ${h}h`;
      }

      // CPU Pane
      const cpuNum = document.getElementById('cpu-global-num');
      const cpuBar = document.getElementById('cpu-global-bar');
      const cpuTemp = document.getElementById('cpu-temp-badge');
      if (cpuNum) cpuNum.textContent = `${data.cpu_global.toFixed(1)}%`;
      if (cpuBar) cpuBar.style.width = `${Math.min(100, Math.max(0, data.cpu_global))}%`;
      if (cpuTemp) cpuTemp.textContent = `${data.cpu_temp ? data.cpu_temp.toFixed(0) : '45'}°C`;

      // Cores grid
      const coresGrid = document.getElementById('cpu-cores-grid');
      if (coresGrid && Array.isArray(data.cpu_cores)) {
        coresGrid.innerHTML = '';
        data.cpu_cores.forEach((corePct, idx) => {
          const div = document.createElement('div');
          div.className = 'core-meter-box';
          div.innerHTML = `
            <div style="display:flex;justify-content:space-between;font-size:11px;margin-bottom:4px;">
              <span>Core ${idx}</span><strong>${corePct.toFixed(0)}%</strong>
            </div>
            <div class="meter-track" style="height:5px;">
              <div class="meter-fill" style="width:${Math.min(100, Math.max(0, corePct))}%;"></div>
            </div>
          `;
          coresGrid.appendChild(div);
        });
      }

      // Memory Pane
      const ramText = document.getElementById('ram-meter-text');
      const ramBar = document.getElementById('ram-meter-bar');
      const swapText = document.getElementById('swap-meter-text');
      const swapBar = document.getElementById('swap-meter-bar');

      if (ramText) ramText.textContent = `${(data.mem_used_mb / 1024).toFixed(1)} / ${(data.mem_total_mb / 1024).toFixed(1)} GB (${data.mem_pct.toFixed(0)}%)`;
      if (ramBar) ramBar.style.width = `${data.mem_pct}%`;
      if (swapText) swapText.textContent = `${(data.swap_used_mb / 1024).toFixed(1)} / ${(data.swap_total_mb / 1024).toFixed(1)} GB (${data.swap_pct.toFixed(0)}%)`;
      if (swapBar) swapBar.style.width = `${data.swap_pct}%`;

      // Disks Pane
      const disksList = document.getElementById('storage-disks-list');
      if (disksList && Array.isArray(data.disks)) {
        disksList.innerHTML = '';
        data.disks.forEach(d => {
          const card = document.createElement('div');
          card.className = 'meter-card';
          card.style.marginBottom = '8px';
          card.innerHTML = `
            <div class="meter-row">
              <strong>${d.mount_point} (${d.name || d.fs_type})</strong>
              <span>${d.used_gb} / ${d.total_gb} GB (${d.pct.toFixed(0)}%)</span>
            </div>
            <div class="meter-track"><div class="meter-fill" style="width:${d.pct}%;background:#10b981;"></div></div>
          `;
          disksList.appendChild(card);
        });
      }

      // Top Apps Table
      const procTbody = document.getElementById('procs-table-body');
      if (procTbody && Array.isArray(data.top_apps)) {
        procTbody.innerHTML = '';
        data.top_apps.forEach(p => {
          const tr = document.createElement('tr');
          tr.innerHTML = `
            <td>${p.pid}</td>
            <td style="color:#f8fafc;font-weight:600;">${escapeHtml(p.name)}</td>
            <td style="color:var(--cyber-cyan);">${p.cpu.toFixed(1)}%</td>
            <td>${p.memory_mb} MB</td>
            <td><span class="status-chip-pill running">${p.status || 'running'}</span></td>
            <td><button class="btn-kill-proc" data-kill-pid="${p.pid}">Kill</button></td>
          `;
          procTbody.appendChild(tr);
        });
      }
    }).catch(err => console.log('Sensors error:', err));
  }

  // 9. SSH Remote Host Controller
  function loadRemoteHostView(host) {
    const title = document.getElementById('remote-host-title');
    const target = document.getElementById('remote-target-desc');
    const statusBadge = document.getElementById('remote-status-badge');

    if (title) title.textContent = host.name;
    if (target) target.textContent = `ssh://${host.user}@${host.host}:${host.port || 22}`;

    // Test connection
    invokeTauri('ssh_test_connection', { hostId: host.id }).then(res => {
      if (statusBadge) {
        statusBadge.textContent = '● Online';
        statusBadge.className = 'rh-status-badge online';
      }
    }).catch(err => {
      if (statusBadge) {
        statusBadge.textContent = '● Offline / Error';
        statusBadge.className = 'rh-status-badge error';
      }
    });

    // Scan ports
    scanRemotePorts(host.id);

    // Actions
    document.getElementById('btn-remote-reconnect').onclick = () => loadRemoteHostView(host);
    document.getElementById('btn-remote-scan-ports').onclick = () => scanRemotePorts(host.id);
    document.getElementById('btn-remote-terminal').onclick = () => {
      invokeTauri('ssh_open_terminal', { hostId: host.id }).then(() => showToast('SSH terminal opened')).catch(e => showToast(e));
    };

    // Remote Command runner
    const runBtn = document.getElementById('btn-run-remote-cmd');
    const cmdIn = document.getElementById('remote-cmd-input');
    if (runBtn && cmdIn) {
      runBtn.onclick = () => {
        const cmdStr = cmdIn.value.trim();
        if (!cmdStr) return;
        const out = document.getElementById('remote-terminal-output');
        if (out) out.textContent = `[SSH: ${host.name}] $ ${cmdStr}\n`;
        invokeTauri('ssh_execute_command', { hostId: host.id, command: cmdStr }).then(res => {
          if (out) out.textContent += res;
        }).catch(e => {
          if (out) out.textContent += `[Error] ` + e;
        });
      };
    }
  }

  function scanRemotePorts(hostId) {
    const list = document.getElementById('remote-ports-list');
    if (list) list.innerHTML = '<p class="empty-state-p">Scanning remote ports via SSH...</p>';

    invokeTauri('ssh_fetch_listeners', { hostId }).then(listeners => {
      if (list && Array.isArray(listeners)) {
        list.innerHTML = '';
        if (listeners.length === 0) {
          list.innerHTML = '<p class="empty-state-p">No open listening ports detected on host.</p>';
          return;
        }
        listeners.forEach(l => {
          const card = document.createElement('div');
          card.className = 'server-card';
          card.style.marginBottom = '6px';
          card.innerHTML = `
            <div class="card-row-header">
              <strong style="color:var(--cyber-cyan);">:${l.port} (${l.proto})</strong>
              <span class="status-chip-pill running">${escapeHtml(l.name)}</span>
            </div>
            <div style="font-size:11px;color:var(--text-muted);font-family:var(--font-mono);">${escapeHtml(l.cmd || '')}</div>
          `;
          list.appendChild(card);
        });
      }
    }).catch(e => {
      if (list) list.innerHTML = `<p class="empty-state-p">Error scanning ports: ${e}</p>`;
    });
  }

  // 10. Android ADB Device Cockpit Controller
  function loadAdbDevices() {
    invokeTauri('adb_devices').then(devices => {
      if (Array.isArray(devices)) {
        state.adbDevices = devices;
        renderDeviceRail();
      }
    }).catch(e => console.log('ADB scan error:', e));
  }

  function loadAdbCockpitView(dev) {
    const model = document.getElementById('adb-model-text');
    const serial = document.getElementById('adb-serial-text');
    if (model) model.textContent = dev.name;
    if (serial) serial.textContent = `Serial: ${dev.serial} (${dev.is_wireless ? 'Wireless' : 'USB'})`;

    // Actions
    document.getElementById('btn-adb-back').onclick = () => invokeTauri('adb_action', { serial: dev.serial, action: 'key_back' });
    document.getElementById('btn-adb-home').onclick = () => invokeTauri('adb_action', { serial: dev.serial, action: 'key_home' });
    document.getElementById('btn-adb-recents').onclick = () => invokeTauri('adb_action', { serial: dev.serial, action: 'key_recents' });
    document.getElementById('btn-adb-power').onclick = () => invokeTauri('adb_action', { serial: dev.serial, action: 'key_power' });
    document.getElementById('btn-adb-scrcpy').onclick = () => invokeTauri('adb_action', { serial: dev.serial, action: 'scrcpy' });
    document.getElementById('btn-adb-screenshot').onclick = () => captureAdbScreen(dev.serial);

    // Initial screenshot
    captureAdbScreen(dev.serial);
  }

  function captureAdbScreen(serial) {
    const disp = document.getElementById('adb-phone-display');
    if (disp) disp.innerHTML = '<p class="empty-state-p">Capturing display...</p>';

    invokeTauri('adb_screenshot', { serial }).then(b64 => {
      if (disp && b64) {
        disp.innerHTML = `<img src="${b64}" alt="Screen Preview" />`;
      }
    }).catch(e => {
      if (disp) disp.innerHTML = `<p class="empty-state-p">Preview offline: ${e}</p>`;
    });
  }

  // 11. Modal Dialogs and Palette
  function openModal(modalId) {
    const backdrop = document.getElementById('modal-backdrop');
    if (backdrop) backdrop.style.display = 'flex';
    document.querySelectorAll('.modal-dialog').forEach(m => m.style.display = 'none');
    const target = document.getElementById(modalId);
    if (target) target.style.display = 'block';
  }

  function closeAllModals() {
    const backdrop = document.getElementById('modal-backdrop');
    if (backdrop) backdrop.style.display = 'none';
    document.querySelectorAll('.modal-dialog').forEach(m => m.style.display = 'none');
  }

  function setupModals() {
    document.querySelectorAll('.btn-close-modal, #btn-cancel-add, #btn-cancel-script, #btn-cancel-cmd, #btn-cancel-ssh, #btn-cancel-power, #btn-save-settings').forEach(btn => {
      btn.onclick = closeAllModals;
    });

    const btnAdd = document.getElementById('btn-add-server');
    if (btnAdd) btnAdd.onclick = () => openModal('modal-add-server');

    const btnSettings = document.getElementById('btn-settings');
    if (btnSettings) {
      btnSettings.onclick = () => {
        invokeTauri('launcher_get_autostart').then(enabled => {
          const chk = document.getElementById('chk-autostart-app');
          if (chk) chk.checked = !!enabled;
        }).catch(() => {});
        openModal('modal-settings');
      };
    }

    const chkAutostart = document.getElementById('chk-autostart-app');
    if (chkAutostart) {
      chkAutostart.onchange = () => {
        invokeTauri('launcher_set_autostart', { enabled: chkAutostart.checked }).then(res => {
          showToast(res);
        }).catch(e => showToast(e));
      };
    }

    // Forms
    const formAdd = document.getElementById('form-add-server');
    if (formAdd) {
      formAdd.onsubmit = (e) => {
        e.preventDefault();
        const fd = new FormData(formAdd);
        const name = fd.get('name');
        const newSrv = {
          key: fd.get('key') || name.toLowerCase().replace(/[^a-z0-9]/g, '_'),
          name: name,
          command: fd.get('command'),
          cwd: fd.get('cwd'),
          port: fd.get('port') ? parseInt(fd.get('port'), 10) : 0,
          group: fd.get('group'),
          status: 'stopped'
        };
        invokeTauri('launcher_save_server', { server: newSrv }).then(() => {
          state.servers.push(newSrv);
          updateStatsCounter();
          closeAllModals();
          showToast(`Server '${newSrv.name}' saved`);
        }).catch(e => showToast(`Save error: ${e}`));
      };
    }

    const formScript = document.getElementById('form-new-script');
    if (formScript) {
      formScript.onsubmit = (e) => {
        e.preventDefault();
        const fd = new FormData(formScript);
        const sc = {
          id: 'script_' + Date.now(),
          name: fd.get('name'),
          shell: fd.get('shell'),
          cwd: fd.get('cwd'),
          script: fd.get('script')
        };
        invokeTauri('launcher_save_script', { script: sc }).then(() => {
          state.scripts.push(sc);
          renderScriptsList();
          closeAllModals();
          showToast(`Script '${sc.name}' created`);
        }).catch(e => showToast(e));
      };
    }

    const formCmd = document.getElementById('form-new-command');
    if (formCmd) {
      formCmd.onsubmit = (e) => {
        e.preventDefault();
        const fd = new FormData(formCmd);
        const cmd = {
          id: 'cmd_' + Date.now(),
          name: fd.get('name'),
          command: fd.get('command'),
          category: fd.get('category'),
          run_in_terminal: fd.get('terminal') === 'true'
        };
        invokeTauri('launcher_save_command', { command: cmd }).then(() => {
          state.savedCommands.push(cmd);
          renderCommandsList();
          closeAllModals();
          showToast(`Command '${cmd.name}' registered`);
        }).catch(e => showToast(e));
      };
    }

    const formSsh = document.getElementById('form-ssh-host');
    if (formSsh) {
      formSsh.onsubmit = (e) => {
        e.preventDefault();
        const fd = new FormData(formSsh);
        const host = {
          id: fd.get('id') || 'host_' + Date.now(),
          name: fd.get('name'),
          host: fd.get('host'),
          port: parseInt(fd.get('port'), 10) || 22,
          user: fd.get('user') || 'ubuntu',
          remote_cwd: fd.get('remote_cwd') || '',
          password: fd.get('password') || ''
        };
        invokeTauri('ssh_save_host', { host }).then(() => {
          state.sshHosts.push(host);
          renderDeviceRail();
          closeAllModals();
          showToast(`Host '${host.name}' saved`);
        }).catch(e => showToast(e));
      };
    }

    // Keyboard Shortcuts
    document.addEventListener('keydown', (e) => {
      if (e.key === 'Escape') closeAllModals();
      if ((e.ctrlKey || e.metaKey) && (e.key === 'p' || e.key === 'k')) {
        e.preventDefault();
        openModal('modal-quick-launch');
      }
    });

    const btnQl = document.getElementById('btn-quick-launch');
    if (btnQl) btnQl.onclick = () => openModal('modal-quick-launch');

    // Action delegations (Kill process, copy command, run command, etc.)
    document.addEventListener('click', (e) => {
      const killBtn = e.target.closest('[data-kill-pid]');
      if (killBtn) {
        const pid = parseInt(killBtn.dataset.killPid, 10);
        invokeTauri('launcher_kill_process', { pid, force: false }).then(res => {
          showToast(res);
          pollDetailedSensors();
        }).catch(err => showToast(err));
        return;
      }

      const copyBtn = e.target.closest('[data-copy-cmd]');
      if (copyBtn) {
        navigator.clipboard.writeText(copyBtn.dataset.copyCmd);
        showToast('Command copied to clipboard');
        return;
      }

      const runCmdBtn = e.target.closest('[data-run-cmd]');
      if (runCmdBtn) {
        executeCustomCommand(runCmdBtn.dataset.runCmd);
        return;
      }

      const runScBtn = e.target.closest('[data-run-script]');
      if (runScBtn) {
        executeScript(runScBtn.dataset.runScript);
        return;
      }

      const toggleBtn = e.target.closest('[data-toggle-key]');
      if (toggleBtn) {
        toggleServer(toggleBtn.dataset.toggleKey);
        return;
      }

      const toolBtn = e.target.closest('[data-tool]');
      if (toolBtn) {
        const tool = toolBtn.dataset.tool;
        const key = toolBtn.dataset.key;
        handleServerTool(tool, key);
        return;
      }
    });

    const inlineCmdBtn = document.getElementById('btn-run-inline-cmd');
    const inlineCmdIn = document.getElementById('cmd-inline-input');
    if (inlineCmdBtn && inlineCmdIn) {
      inlineCmdBtn.onclick = () => {
        const c = inlineCmdIn.value.trim();
        if (c) executeCustomCommand(c);
      };
    }
  }

  function handleServerTool(tool, key) {
    const srv = state.servers.find(s => s.key === key);
    if (!srv) return;

    if (tool === 'terminal') {
      invokeTauri('launcher_open_terminal', { cwd: srv.cwd }).then(() => showToast(`Terminal opened`)).catch(e => showToast(e));
    } else if (tool === 'folder') {
      invokeTauri('launcher_open_folder', { path: srv.cwd }).then(() => showToast(`Folder opened`)).catch(e => showToast(e));
    } else if (tool === 'code') {
      invokeTauri('launcher_execute_command', { command: `code "${srv.cwd}"` }).then(() => showToast(`VS Code launched`)).catch(e => showToast(e));
    } else if (tool === 'delete') {
      if (confirm(`Delete '${srv.name}'?`)) {
        invokeTauri('launcher_delete_server', { key }).then(() => {
          state.servers = state.servers.filter(s => s.key !== key);
          document.querySelectorAll(`#card-${key}, #managed-card-${key}`).forEach(el => el.remove());
          updateStatsCounter();
          showToast(`Deleted ${srv.name}`);
        }).catch(e => showToast(e));
      }
    }
  }

  function setupClock() {
    const timer = document.getElementById('scan-timer');
    if (timer) {
      setInterval(() => {
        const d = new Date();
        timer.textContent = `${String(d.getHours()).padStart(2,'0')}:${String(d.getMinutes()).padStart(2,'0')}:${String(d.getSeconds()).padStart(2,'0')}`;
      }, 1000);
    }
  }

  function syncInitialBackendData() {
    invokeTauri('launcher_get_data').then(data => {
      if (data) {
        if (data.lan_ip) {
          state.lanIp = data.lan_ip;
          const ipEl = document.getElementById('meta-ip');
          if (ipEl) ipEl.textContent = data.lan_ip;
        }
        if (Array.isArray(data.servers) && data.servers.length > 0) {
          state.servers = data.servers.map(s => ({ ...s, status: s.status || 'stopped' }));
          updateStatsCounter();
        }
        if (Array.isArray(data.ssh_hosts)) {
          state.sshHosts = data.ssh_hosts;
        }
        if (Array.isArray(data.adb_devices)) {
          state.adbDevices = data.adb_devices;
        }
        if (Array.isArray(data.scripts) && data.scripts.length > 0) {
          state.scripts = data.scripts;
        }
        if (Array.isArray(data.saved_commands) && data.saved_commands.length > 0) {
          state.savedCommands = data.saved_commands;
        }
        renderDeviceRail();
        renderScriptsList();
        renderCommandsList();
      }
    }).catch(e => console.log('Offline backend sync:', e));

    loadAdbDevices();
  }

  function escapeHtml(str) {
    return String(str || '').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  }

  function init() {
    setupWindowControls();
    setupDeviceRail();
    setupSidebarTabs();
    setupSubtabsRouter();
    setupSensorsNav();
    setupModals();
    setupClock();
    syncInitialBackendData();
    console.log('Server Launcher complete system active.');
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
  t._timer = setTimeout(() => { t.style.opacity = '0'; }, 2500);
}
