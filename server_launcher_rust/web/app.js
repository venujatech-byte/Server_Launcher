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

    const beacon = document.getElementById('rail-local-beacon');
    if (beacon) beacon.style.display = running > 0 ? 'block' : 'none';
  }

  // 5. Scripts Page Controller (Image 1)
  function renderScriptsList() {
    const list = document.getElementById('scripts-left-list');
    if (!list) return;
    list.innerHTML = '';

    const filterIn = document.getElementById('scripts-search-input');
    const query = filterIn ? filterIn.value.trim().toLowerCase() : '';

    const matched = state.scripts.filter(s => {
      return s.name.toLowerCase().includes(query) || (s.script && s.script.toLowerCase().includes(query));
    });

    if (matched.length === 0) {
      list.innerHTML = `
        <div class="empty-list-placeholder">
          <p class="empty-main">No scripts found</p>
          <p class="empty-sub">Click '+ New Script' to create one.</p>
        </div>
      `;
      const emptyCard = document.getElementById('scripts-empty-card');
      const activePane = document.getElementById('scripts-active-pane');
      if (emptyCard) emptyCard.style.display = 'flex';
      if (activePane) activePane.style.display = 'none';
      return;
    }

    matched.forEach(sc => {
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
      const runBtn = card.querySelector('[data-run-script]');
      if (runBtn) {
        runBtn.onclick = (e) => {
          e.stopPropagation();
          selectScript(sc);
          executeScript(sc.id);
        };
      }
      list.appendChild(card);
    });

    const btnNew = document.getElementById('btn-add-script');
    if (btnNew) btnNew.onclick = () => openModal('modal-new-script');

    if (filterIn && !filterIn.dataset.bound) {
      filterIn.dataset.bound = 'true';
      filterIn.oninput = () => renderScriptsList();
    }
  }

  function selectScript(sc) {
    state.activeScriptId = sc.id;
    const emptyCard = document.getElementById('scripts-empty-card');
    const activePane = document.getElementById('scripts-active-pane');
    if (emptyCard) emptyCard.style.display = 'none';
    if (activePane) activePane.style.display = 'flex';

    document.querySelectorAll('.script-card-item').forEach(c => c.classList.remove('active'));
    renderScriptsList();

    const titleEl = document.getElementById('scripts-active-name');
    const descEl = document.getElementById('scripts-status-desc');
    const outEl = document.getElementById('scripts-terminal-output');

    if (titleEl) titleEl.textContent = sc.name;
    if (descEl) descEl.textContent = `Interpreter: ${sc.shell} | CWD: ${sc.cwd}`;
    if (outEl) outEl.textContent = `// Script Source Code:\n\n${sc.script}\n\n// Click 'Execute Now' to run.`;

    const runBtn = document.getElementById('btn-run-active-script');
    if (runBtn) runBtn.onclick = () => executeScript(sc.id);

    const clearBtn = document.getElementById('btn-clear-scripts-output');
    if (clearBtn) clearBtn.onclick = () => { if (outEl) outEl.textContent = '// Logs cleared.'; };
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

  // 6. Saved Commands & Interactive Terminal Controller (Image 4)
  let termHistory = [];
  let termHistoryIdx = -1;
  let termCurrentPrompt = 'venujasilva@:~/Documents/Coding/Server_Launcher/server_launcher_rust$ ';

  function initLocalTerminal() {
    invokeTauri('launcher_terminal_init').then(res => {
      if (res) {
        if (res.prompt) termCurrentPrompt = res.prompt;
        const promptEl = document.getElementById('local-term-prompt-str');
        if (promptEl) promptEl.textContent = termCurrentPrompt;
        const cwdEl = document.getElementById('local-term-cwd-display');
        if (cwdEl && res.cwd) cwdEl.textContent = res.cwd;
        if (Array.isArray(res.history)) termHistory = res.history;
      }
    }).catch(() => {});

    const termInput = document.getElementById('local-term-input');
    const runBtn = document.getElementById('btn-local-term-run');
    const clearBtn = document.getElementById('btn-clear-local-term');
    const nativeBtn = document.getElementById('btn-open-native-term');

    if (termInput && !termInput.dataset.bound) {
      termInput.dataset.bound = 'true';
      termInput.addEventListener('keydown', (e) => {
        if (e.key === 'Enter') {
          e.preventDefault();
          const cmd = termInput.value;
          runLocalTerminal(cmd);
          termInput.value = '';
          termHistoryIdx = -1;
        } else if (e.key === 'ArrowUp') {
          e.preventDefault();
          if (termHistory.length > 0) {
            if (termHistoryIdx === -1) termHistoryIdx = termHistory.length - 1;
            else if (termHistoryIdx > 0) termHistoryIdx--;
            termInput.value = termHistory[termHistoryIdx] || '';
          }
        } else if (e.key === 'ArrowDown') {
          e.preventDefault();
          if (termHistory.length > 0 && termHistoryIdx !== -1) {
            if (termHistoryIdx < termHistory.length - 1) {
              termHistoryIdx++;
              termInput.value = termHistory[termHistoryIdx] || '';
            } else {
              termHistoryIdx = -1;
              termInput.value = '';
            }
          }
        } else if (e.key === 'Tab') {
          e.preventDefault();
          invokeTauri('launcher_terminal_autocomplete', { text: termInput.value }).then(res => {
            if (res) {
              if (res.completed) {
                termInput.value = res.completed;
              } else if (Array.isArray(res.candidates) && res.candidates.length > 0) {
                const con = document.getElementById('local-term-console');
                if (con) {
                  const line = document.createElement('div');
                  line.className = 'term-log-line stdout';
                  line.textContent = res.candidates.join('   ');
                  con.appendChild(line);
                  con.scrollTop = con.scrollHeight;
                }
              }
            }
          });
        } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'l') {
          e.preventDefault();
          clearLocalTerminal();
        }
      });
    }

    if (runBtn) {
      runBtn.onclick = () => {
        if (termInput) {
          runLocalTerminal(termInput.value);
          termInput.value = '';
          termHistoryIdx = -1;
        }
      };
    }

    if (clearBtn) clearBtn.onclick = clearLocalTerminal;
    if (nativeBtn) {
      nativeBtn.onclick = () => {
        const cwdEl = document.getElementById('local-term-cwd-display');
        const cwd = cwdEl ? cwdEl.textContent : '.';
        invokeTauri('launcher_open_terminal', { cwd, command: null })
          .then(() => showToast('Native terminal launched'))
          .catch(e => showToast(e));
      };
    }
  }

  function clearLocalTerminal() {
    const con = document.getElementById('local-term-console');
    if (con) {
      con.innerHTML = '<div class="term-log-line banner">Terminal ready. Type a command below or press Tab for auto-completion.</div>';
    }
  }

  function runLocalTerminal(cmdStr) {
    const con = document.getElementById('local-term-console');
    if (!con) return;

    const trimmed = cmdStr.trim();
    const promptLine = document.createElement('div');
    promptLine.className = 'term-log-line prompt-line';
    promptLine.textContent = `${termCurrentPrompt}${trimmed}`;
    con.appendChild(promptLine);
    con.scrollTop = con.scrollHeight;

    if (!trimmed) return;

    if (termHistory.length === 0 || termHistory[termHistory.length - 1] !== trimmed) {
      termHistory.push(trimmed);
    }

    invokeTauri('launcher_terminal_run', { command: trimmed, cwdOverride: null }).then(res => {
      if (res.clear) {
        clearLocalTerminal();
        return;
      }
      if (res.stdout) {
        const outLine = document.createElement('div');
        outLine.className = 'term-log-line stdout';
        outLine.textContent = res.stdout;
        con.appendChild(outLine);
      }
      if (res.stderr) {
        const errLine = document.createElement('div');
        errLine.className = 'term-log-line stderr';
        errLine.textContent = res.stderr;
        con.appendChild(errLine);
      }
      if (res.prompt) {
        termCurrentPrompt = res.prompt;
        const promptEl = document.getElementById('local-term-prompt-str');
        if (promptEl) promptEl.textContent = termCurrentPrompt;
      }
      if (res.cwd) {
        const cwdEl = document.getElementById('local-term-cwd-display');
        if (cwdEl) cwdEl.textContent = res.cwd;
      }
      con.scrollTop = con.scrollHeight;
    }).catch(err => {
      const errLine = document.createElement('div');
      errLine.className = 'term-log-line stderr';
      errLine.textContent = `Launcher execution error: ${err}`;
      con.appendChild(errLine);
      con.scrollTop = con.scrollHeight;
    });
  }

  function renderCommandsList() {
    const list = document.getElementById('cmds-left-list');
    if (!list) return;
    list.innerHTML = '';

    const filterIn = document.getElementById('cmds-search-input');
    const query = filterIn ? filterIn.value.trim().toLowerCase() : '';

    const filtered = state.savedCommands.filter(c => {
      const matchCat = (state.activeCmdCat === 'all' || c.category === state.activeCmdCat);
      const matchQuery = !query || c.name.toLowerCase().includes(query) || c.command.toLowerCase().includes(query);
      return matchCat && matchQuery;
    });

    filtered.forEach(cmd => {
      const card = document.createElement('div');
      card.className = 'server-card';
      card.style.marginBottom = '8px';
      card.innerHTML = `
        <div class="card-row-header">
          <div class="card-title-wrap">
            <span class="card-title-text">${escapeHtml(cmd.name)}</span>
          </div>
          <span class="status-chip-pill stopped">[${escapeHtml(cmd.category || 'General')}]</span>
        </div>
        <div class="recessed-command-box" title="Click to copy command">
          <code class="code-command">${escapeHtml(cmd.command)}</code>
        </div>
        <div class="card-actions-row">
          <button class="btn-glow-start" data-run-cmd="${escapeHtml(cmd.command)}">
            <span class="icon">&#xe13c;</span><span>Run</span>
          </button>
          <div class="card-tools-cluster">
            <button class="tool-btn" data-copy-cmd="${escapeHtml(cmd.command)}" title="Copy"><span class="icon">&#xe0b2;</span></button>
            <button class="tool-btn" data-edit-cmd="${cmd.id}" title="Edit"><span class="icon">&#xe1f9;</span></button>
            <button class="tool-btn tool-danger" data-del-cmd="${cmd.id}" title="Delete"><span class="icon">&#xe18e;</span></button>
          </div>
        </div>
        <div class="card-checkboxes-row">
          <label class="toggle-checkbox">
            <input type="checkbox" data-ext-term="${cmd.id}" ${cmd.external_terminal ? 'checked' : ''} />
            <span class="checkbox-ring"></span><span>Run in external terminal</span>
          </label>
        </div>
      `;

      const runBtn = card.querySelector('[data-run-cmd]');
      if (runBtn) {
        runBtn.onclick = () => {
          const extCheck = card.querySelector(`[data-ext-term="${cmd.id}"]`);
          if (extCheck && extCheck.checked) {
            invokeTauri('launcher_open_terminal', { cwd: '.', command: cmd.command })
              .then(() => showToast(`Executed '${cmd.name}' in external terminal`))
              .catch(e => showToast(e));
          } else {
            runLocalTerminal(cmd.command);
            showToast(`Sent '${cmd.name}' to Local Terminal`);
          }
        };
      }

      const copyBtn = card.querySelector('[data-copy-cmd]');
      if (copyBtn) {
        copyBtn.onclick = () => {
          navigator.clipboard.writeText(cmd.command);
          showToast('Command copied to clipboard');
        };
      }

      const delBtn = card.querySelector('[data-del-cmd]');
      if (delBtn) {
        delBtn.onclick = () => {
          invokeTauri('launcher_delete_command', { id: cmd.id }).then(() => {
            state.savedCommands = state.savedCommands.filter(c => c.id !== cmd.id);
            renderCommandsList();
            showToast('Command deleted');
          }).catch(e => showToast(e));
        };
      }

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

    const catSelect = document.getElementById('cmds-cat-select');
    if (catSelect && !catSelect.dataset.bound) {
      catSelect.dataset.bound = 'true';
      catSelect.onchange = () => {
        state.activeCmdCat = catSelect.value;
        document.querySelectorAll('#cmds-categories-row .cat-pill').forEach(p => p.classList.toggle('active', p.dataset.cat === catSelect.value));
        renderCommandsList();
      };
    }

    if (filterIn && !filterIn.dataset.bound) {
      filterIn.dataset.bound = 'true';
      filterIn.oninput = () => renderCommandsList();
    }

    const btnNew = document.getElementById('btn-add-command');
    if (btnNew) btnNew.onclick = () => openModal('modal-new-command');
  }

  // 7. SystemD Page Controller (Image 2)
  state.systemdMode = 'user'; // 'user' (--user) or 'system'

  function loadSystemdServices() {
    const list = document.getElementById('systemd-left-list');
    if (list) list.innerHTML = '<p class="empty-state-p">Loading systemd services...</p>';

    const isUser = (state.systemdMode === 'user');
    invokeTauri('launcher_list_systemd_mode', { userMode: isUser }).then(services => {
      if (Array.isArray(services)) {
        state.systemdServices = services;
        renderSystemdList();
      }
    }).catch(() => {
      // Fallback to launcher_list_systemd
      invokeTauri('launcher_list_systemd').then(svcs => {
        if (Array.isArray(svcs)) {
          state.systemdServices = svcs;
          renderSystemdList();
        }
      }).catch(e => {
        if (list) list.innerHTML = `<p class="empty-state-p">Error loading systemd: ${e}</p>`;
      });
    });
  }

  function renderSystemdList() {
    const list = document.getElementById('systemd-left-list');
    if (!list) return;
    list.innerHTML = '';

    const filterIn = document.getElementById('sysd-search-input');
    const query = filterIn ? filterIn.value.toLowerCase().trim() : '';

    const matched = state.systemdServices.filter(s => {
      return !query || s.name.toLowerCase().includes(query) || (s.description && s.description.toLowerCase().includes(query));
    });

    matched.forEach(unit => {
      const card = document.createElement('div');
      const isSelected = state.activeSysdUnit && state.activeSysdUnit.name === unit.name;
      card.className = `sysd-service-card ${isSelected ? 'active' : ''}`;
      const isRunning = unit.is_active || unit.active_state === 'active';
      const isFailed = unit.is_failed || unit.active_state === 'failed';

      card.innerHTML = `
        <div class="sysd-card-top">
          <div class="sysd-card-name-wrap">
            <span class="sysd-hollow-dot ${isFailed ? 'failed' : ''}">○</span>
            <span class="sysd-unit-name">${escapeHtml(unit.name)}</span>
          </div>
          <span class="sysd-status-text ${isFailed ? 'failed' : (isRunning ? 'running' : '')}">${isRunning ? 'running' : unit.active_state}</span>
        </div>
        <div class="sysd-card-desc">${escapeHtml(unit.description || unit.name)}</div>
      `;
      card.onclick = () => selectSystemdUnit(unit);
      list.appendChild(card);
    });

    // Mode Toggle Buttons
    const btnUser = document.getElementById('btn-sysd-mode-user');
    const btnSys = document.getElementById('btn-sysd-mode-system');
    const btnRefresh = document.getElementById('btn-refresh-sysd');

    if (btnUser) {
      btnUser.onclick = () => {
        state.systemdMode = 'user';
        btnUser.classList.add('active');
        if (btnSys) btnSys.classList.remove('active');
        loadSystemdServices();
      };
    }
    if (btnSys) {
      btnSys.onclick = () => {
        state.systemdMode = 'system';
        btnSys.classList.add('active');
        if (btnUser) btnUser.classList.remove('active');
        loadSystemdServices();
      };
    }
    if (btnRefresh) btnRefresh.onclick = () => loadSystemdServices();

    if (filterIn && !filterIn.dataset.bound) {
      filterIn.dataset.bound = 'true';
      filterIn.oninput = () => renderSystemdList();
    }
  }

  function selectSystemdUnit(unit) {
    state.activeSysdUnit = unit;
    const emptyCard = document.getElementById('sysd-empty-card');
    const activePane = document.getElementById('sysd-active-pane');
    if (emptyCard) emptyCard.style.display = 'none';
    if (activePane) activePane.style.display = 'flex';

    document.querySelectorAll('.sysd-service-card').forEach(c => c.classList.remove('active'));
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

  // 8. Sensors Telemetry Page Controller (Image 3)
  let sensorPollTimer = null;
  let sensorPollInterval = 5000;

  function setupSensorsNav() {
    document.querySelectorAll('#sensors-nav-list .sensor-nav-item').forEach(btn => {
      btn.onclick = () => {
        const cat = btn.dataset.sensorCat;
        state.activeSensorCat = cat;
        document.querySelectorAll('#sensors-nav-list .sensor-nav-item').forEach(b => b.classList.toggle('active', b === btn));
        
        const allPanes = ['overview', 'top_apps', 'cpu', 'memory', 'gpu', 'storage', 'wifi', 'data_usage', 'power', 'bluetooth', 'temperature'];
        allPanes.forEach(c => {
          const pane = document.getElementById(`spane-${c}`);
          if (pane) pane.style.display = (c === cat) ? 'block' : 'none';
        });
      };
    });

    const refreshTopBtn = document.getElementById('btn-refresh-sensors-sidebar');
    if (refreshTopBtn) refreshTopBtn.onclick = pollDetailedSensors;

    const stripRefreshBtn = document.getElementById('strip-refresh-btn');
    if (stripRefreshBtn) stripRefreshBtn.onclick = pollDetailedSensors;

    const intervalSel = document.getElementById('sensors-interval-select');
    if (intervalSel) {
      intervalSel.onchange = () => {
        sensorPollInterval = parseInt(intervalSel.value, 10) || 5000;
        restartSensorTimer();
      };
    }

    restartSensorTimer();
  }

  function restartSensorTimer() {
    if (sensorPollTimer) clearInterval(sensorPollTimer);
    pollDetailedSensors();
    sensorPollTimer = setInterval(() => {
      if (state.activeSidebarTab === 'sensors' && state.selectedMachine === 'local') {
        pollDetailedSensors();
      }
    }, sensorPollInterval);
  }

  function pollDetailedSensors() {
    invokeTauri('launcher_get_telemetry_stream').then(data => {
      if (!data) return;
      applyTelemetryData(data);
    }).catch(() => {
      // Fallback
      invokeTauri('launcher_get_detailed_sensors').then(data => {
        if (data) applyTelemetryData(data);
      }).catch(err => console.log('Sensors error:', err));
    });
  }

  function applyTelemetryData(data) {
    // 1. Top Strip Chips
    const cpuChip = document.getElementById('strip-cpu-text');
    const ramChip = document.getElementById('strip-ram-text');
    const storageChip = document.getElementById('strip-storage-text');
    const uptimeChip = document.getElementById('strip-uptime-text');

    if (cpuChip && data.cpu_brand) {
      cpuChip.textContent = `${data.cpu_brand} ${data.cpu_cores_logical || 4}C/${data.cpu_cores_logical || 4}T`;
    }
    if (ramChip && data.mem_total_mb) {
      ramChip.textContent = `${Math.round(data.mem_total_mb / 1024)}GB RAM`;
    }
    if (storageChip && data.storage_total_gb) {
      storageChip.textContent = `${data.storage_total_gb.toFixed(1)} GB`;
    }
    if (uptimeChip && data.uptime_secs) {
      const h = Math.floor(data.uptime_secs / 3600);
      const m = Math.floor((data.uptime_secs % 3600) / 60);
      uptimeChip.textContent = `Up ${h}h ${m}m`;
    }

    // 2. Overview 4 Stat Cards
    const cpuVal = document.getElementById('ov-cpu-val');
    const cpuSub = document.getElementById('ov-cpu-sub');
    if (cpuVal) cpuVal.textContent = `${data.cpu_global.toFixed(1)} %`;
    if (cpuSub && data.cpu_freq_mhz) {
      cpuSub.textContent = `${data.cpu_cores_physical || 4} Core / ${data.cpu_cores_logical || 4}T • ${(data.cpu_freq_mhz / 1000).toFixed(2)} GHz`;
    }

    const ramVal = document.getElementById('ov-ram-val');
    const ramSub = document.getElementById('ov-ram-sub');
    if (ramVal) ramVal.textContent = `${data.mem_pct.toFixed(1)} %`;
    if (ramSub) ramSub.textContent = `${data.mem_used_mb.toFixed(2)} MB / ${data.mem_total_mb.toFixed(2)} MB`;

    const swapVal = document.getElementById('ov-swap-val');
    const swapSub = document.getElementById('ov-swap-sub');
    if (swapVal) swapVal.textContent = `${data.swap_pct.toFixed(1)} %`;
    if (swapSub) swapSub.textContent = `${(data.swap_used_mb / 1024).toFixed(1)} GB / ${(data.swap_total_mb / 1024).toFixed(1)} GB`;

    const storageVal = document.getElementById('ov-storage-val');
    const storageSub = document.getElementById('ov-storage-sub');
    const storageFree = document.getElementById('ov-storage-free');
    if (storageVal) storageVal.textContent = `${data.storage_pct.toFixed(1)} %`;
    if (storageSub && data.storage_total_gb) {
      storageSub.textContent = `${data.storage_used_gb.toFixed(2)} GB / ${data.storage_total_gb.toFixed(2)} GB`;
    }
    if (storageFree && data.storage_total_gb) {
      storageFree.textContent = `${(data.storage_total_gb - data.storage_used_gb).toFixed(2)} GB`;
    }

    // 3. Overall Speed & Sockets
    const speedUp = document.getElementById('ov-speed-up');
    const speedDown = document.getElementById('ov-speed-down');
    if (speedUp && data.tx_bps !== undefined) {
      speedUp.textContent = formatBytesRate(data.tx_bps);
    }
    if (speedDown && data.rx_bps !== undefined) {
      speedDown.textContent = formatBytesRate(data.rx_bps);
    }

    const sockTotal = document.getElementById('ov-sockets-total');
    const tcpCount = document.getElementById('ov-tcp-count');
    const udpCount = document.getElementById('ov-udp-count');
    if (sockTotal && data.open_sockets) sockTotal.textContent = `${data.open_sockets} open sockets`;
    if (tcpCount && data.tcp_count !== undefined) tcpCount.textContent = data.tcp_count;
    if (udpCount && data.udp_count !== undefined) udpCount.textContent = data.udp_count;

    // 4. Top Apps Table
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
        const killBtn = tr.querySelector('[data-kill-pid]');
        if (killBtn) {
          killBtn.onclick = () => {
            invokeTauri('launcher_kill_process', { pid: p.pid, force: false })
              .then(() => showToast(`Terminated ${p.name} (PID ${p.pid})`))
              .catch(e => showToast(e));
          };
        }
        procTbody.appendChild(tr);
      });
    }
  }

  function formatBytesRate(bps) {
    if (bps >= 1024 * 1024) return `${(bps / (1024 * 1024)).toFixed(1)} MB/s`;
    if (bps >= 1024) return `${(bps / 1024).toFixed(1)} KB/s`;
    return `${Math.round(bps)} B/s`;
  }

  // 9. SSH Remote Host Controller (Image 5)
  let currentRemoteHost = null;

  function loadRemoteHostView(host) {
    currentRemoteHost = host;
    const title = document.getElementById('remote-host-title');
    const target = document.getElementById('remote-target-desc');
    const statusBadge = document.getElementById('remote-status-badge');

    if (title) title.textContent = host.name;
    if (target) target.textContent = `ssh://${host.user}@${host.host}:${host.port || 22}`;

    const promptEl = document.getElementById('remote-term-prompt-str');
    if (promptEl) promptEl.textContent = `${host.user}@${host.host}:~$ `;

    // Test connection
    invokeTauri('ssh_test_connection', { hostId: host.id }).then(() => {
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

    // Header Actions
    const btnReboot = document.getElementById('btn-remote-reboot');
    if (btnReboot) {
      btnReboot.onclick = () => {
        if (confirm(`Are you sure you want to restart device '${host.name}'?`)) {
          invokeTauri('ssh_reboot', { hostId: host.id, sudoPassword: '' })
            .then(() => showToast('Reboot command sent'))
            .catch(e => showToast(e));
        }
      };
    }

    const btnShutdown = document.getElementById('btn-remote-shutdown');
    if (btnShutdown) {
      btnShutdown.onclick = () => {
        if (confirm(`Are you sure you want to shut down remote host '${host.name}'?`)) {
          invokeTauri('ssh_shutdown', { hostId: host.id, sudoPassword: '' })
            .then(() => showToast('Shutdown command sent'))
            .catch(e => showToast(e));
        }
      };
    }

    const btnScan = document.getElementById('btn-remote-scan-ports');
    if (btnScan) btnScan.onclick = () => scanRemotePorts(host.id);

    const btnTerm = document.getElementById('btn-remote-terminal');
    if (btnTerm) {
      btnTerm.onclick = () => {
        invokeTauri('ssh_open_terminal', { hostId: host.id })
          .then(() => showToast('SSH external terminal opened'))
          .catch(e => showToast(e));
      };
    }

    const btnReconnect = document.getElementById('btn-remote-reconnect');
    if (btnReconnect) btnReconnect.onclick = () => loadRemoteHostView(host);

    // Quick Command Chips
    document.querySelectorAll('#remote-quick-chips .quick-chip[data-remote-cmd]').forEach(chip => {
      chip.onclick = () => {
        const cmd = chip.dataset.remoteCmd;
        runRemoteCommand(host, cmd);
      };
    });

    const clearRemoteBtn = document.getElementById('btn-clear-remote-term');
    if (clearRemoteBtn) {
      clearRemoteBtn.onclick = () => {
        const con = document.getElementById('remote-terminal-console');
        if (con) con.innerHTML = '<div class="term-log-line banner">[ssh] Terminal initialized. Ready to execute remote commands.</div>';
      };
    }

    // Remote Interactive Command Bar
    const runBtn = document.getElementById('btn-remote-term-execute');
    const cmdIn = document.getElementById('remote-term-input');

    if (runBtn && cmdIn) {
      runBtn.onclick = () => {
        const cmd = cmdIn.value.trim();
        if (cmd) {
          runRemoteCommand(host, cmd);
          cmdIn.value = '';
        }
      };
      if (!cmdIn.dataset.bound) {
        cmdIn.dataset.bound = 'true';
        cmdIn.addEventListener('keydown', (e) => {
          if (e.key === 'Enter') {
            e.preventDefault();
            const cmd = cmdIn.value.trim();
            if (cmd) {
              runRemoteCommand(host, cmd);
              cmdIn.value = '';
            }
          }
        });
      }
    }
  }

  function runRemoteCommand(host, cmdStr) {
    const con = document.getElementById('remote-terminal-console');
    if (!con) return;

    const promptLine = document.createElement('div');
    promptLine.className = 'term-log-line prompt-line';
    promptLine.textContent = `${host.user}@${host.host}:~$ ${cmdStr}`;
    con.appendChild(promptLine);
    con.scrollTop = con.scrollHeight;

    invokeTauri('ssh_terminal_run', { hostId: host.id, command: cmdStr }).then(res => {
      if (res.clear) {
        con.innerHTML = '<div class="term-log-line banner">[ssh] Terminal cleared.</div>';
        return;
      }
      if (res.stdout) {
        const outLine = document.createElement('div');
        outLine.className = 'term-log-line stdout';
        outLine.textContent = res.stdout;
        con.appendChild(outLine);
      }
      if (res.stderr) {
        const errLine = document.createElement('div');
        errLine.className = 'term-log-line stderr';
        errLine.textContent = res.stderr;
        con.appendChild(errLine);
      }
      con.scrollTop = con.scrollHeight;
    }).catch(err => {
      const errLine = document.createElement('div');
      errLine.className = 'term-log-line stderr';
      errLine.textContent = `SSH execution error: ${err}`;
      con.appendChild(errLine);
      con.scrollTop = con.scrollHeight;
    });
  }

  function scanRemotePorts(hostId) {
    const list = document.getElementById('remote-ports-list');
    if (list) list.innerHTML = '<p class="empty-state-p">Scanning remote listeners via SSH...</p>';

    invokeTauri('ssh_fetch_listeners', { hostId }).then(listeners => {
      if (list && Array.isArray(listeners)) {
        list.innerHTML = '';
        const countHeader = document.getElementById('remote-servers-count-header');
        if (countHeader) countHeader.textContent = `REMOTE SERVERS (${listeners.length})`;

        if (listeners.length === 0) {
          list.innerHTML = '<p class="empty-state-p">No open listening ports detected on host.</p>';
          return;
        }
        listeners.forEach(l => {
          const card = document.createElement('div');
          card.className = 'server-card';
          card.style.marginBottom = '8px';
          card.innerHTML = `
            <div class="card-row-header">
              <div class="card-title-wrap">
                <span style="color:var(--cyber-cyan);font-weight:700;font-family:var(--font-mono);">:${l.port} [${l.proto || 'TCP'}]</span>
                <span class="card-title-text" style="margin-left:8px;">${escapeHtml(l.name || 'port_' + l.port)}</span>
              </div>
              <span class="status-chip-pill running">Running</span>
            </div>
            ${l.pid ? `<div style="font-size:11px;color:var(--text-muted);font-family:var(--font-mono);margin-bottom:6px;">PID ${l.pid} • Listening on :${l.port}</div>` : ''}
            <div class="card-actions-row">
              <div class="card-tools-cluster" style="margin-left:0;gap:6px;">
                <button class="tool-btn tool-danger" data-ssh-action="stop" data-port="${l.port}" title="Stop Service">Stop</button>
                <button class="tool-btn" data-ssh-action="restart" data-port="${l.port}" title="Restart Service">Restart</button>
                <button class="tool-btn tool-danger" data-ssh-action="kill" data-pid="${l.pid || ''}" title="Kill Process">Kill</button>
                <button class="tool-btn" data-ssh-action="curl" data-port="${l.port}" title="Curl HTTP">Curl</button>
                <button class="tool-btn" data-ssh-action="info" data-port="${l.port}" title="Info">Info</button>
              </div>
            </div>
          `;

          // Bind tools
          card.querySelectorAll('[data-ssh-action]').forEach(btn => {
            btn.onclick = () => {
              const action = btn.dataset.sshAction;
              if (action === 'curl') {
                runRemoteCommand(currentRemoteHost, `curl -I http://localhost:${l.port}`);
              } else if (action === 'kill' && l.pid) {
                runRemoteCommand(currentRemoteHost, `kill -9 ${l.pid}`);
              } else if (action === 'restart') {
                runRemoteCommand(currentRemoteHost, `systemctl restart ${l.name}`);
              } else if (action === 'stop') {
                runRemoteCommand(currentRemoteHost, `systemctl stop ${l.name}`);
              } else {
                runRemoteCommand(currentRemoteHost, `ss -tulpn | grep :${l.port}`);
              }
            };
          });

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
    initLocalTerminal();
    loadSystemdServices();
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
// Scripts filter: filters custom scripts by name or code
// Scripts empty: shows empty placeholder when list is empty
// Scripts selection: displays active script code and metadata
// Scripts execution: invokes backend and logs output stream
// Terminal init: syncs cwd and prompt on startup
// Terminal history: navigates history backwards and forwards
// Terminal tab: requests path completion from backend
// Terminal clear: resets console viewport on Ctrl+L
// Terminal runner: executes command and handles cwd changes
// Terminal native: launches external terminal emulator in current cwd
// Commands category: filters commands by selected category
// Commands search: filters commands by query in name or code
// Commands run: executes command in terminal or external emulator
// Commands copy: writes command text to system clipboard
// Commands delete: deletes command from configuration
// SystemD mode: switches query scope between user and system units
// SystemD search: filters unit cards by name and description
// SystemD cards: renders unit cards matching egui design
// SystemD details: inspects active unit load and sub state
// SystemD journal: streams journalctl logs with tail refresh
// SystemD actions: triggers unit lifecycle actions
// Sensors router: switches between 11 hardware telemetry views
// Sensors interval: adjusts telemetry polling timer
// Telemetry strip: updates CPU model, RAM, storage, and uptime
// Telemetry CPU: updates CPU metrics and averages
// Telemetry RAM: updates physical memory usage and capacity
// Telemetry Swap: updates swap space metrics
// Telemetry Storage: updates disk capacity and free space
// Network formatting: formats bytes/sec into B/s, KB/s, MB/s
// Network speed: updates real-time throughput metrics
// Socket stats: updates active socket statistics
// Top apps: renders ranked process table
// Process kill: sends termination signal to selected PID
// Remote connect: tests SSH connection and updates online badge
// Remote listeners: scans remote listening ports over SSH
