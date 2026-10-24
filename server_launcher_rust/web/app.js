
// Server Launcher Reactive Client Controller
(function() {
  'use strict';

  const state = {
    servers: [
      { id: 'vendly', name: 'Vendly Backend', group: 'backend', cmd: 'python run.py', port: null, folder: 'backend', status: 'stopped' },
      { id: 'f', name: 'f', group: 'frontend', cmd: 'npm run dev', port: '5173', folder: 'frontend', status: 'stopped' },
      { id: 'portfolio', name: 'Portfolio', group: 'frontend', cmd: 'npm run dev', port: '3000', folder: 'portfolio', status: 'stopped' },
      { id: 'expo', name: 'React Native (Expo)', group: 'mobile', cmd: 'npx expo start', port: '8081', folder: 'mobile', status: 'stopped' },
      { id: 'analytics', name: 'Analytics Worker', group: 'backend', cmd: 'cargo run', port: '8080', folder: 'analytics', status: 'stopped' }
    ],
    activeTab: 'servers',
    activeGroup: 'all',
    activeSubtab: 'overview',
    searchQuery: '',
    scanElapsedSec: 565
  };

const DOM = {
    navTabs: document.querySelectorAll('.nav-pill-tab'),
    groupPills: document.querySelectorAll('.group-pill'),
    subtabs: document.querySelectorAll('.subtab-item'),
    globalSearch: document.getElementById('global-search'),
    subbarSearch: document.getElementById('subbar-search'),
    scanTimer: document.getElementById('scan-timer'),
    btnRefresh: document.getElementById('btn-refresh'),
    btnQuickLaunch: document.getElementById('btn-quick-launch'),
    btnAddServer: document.getElementById('btn-add-server'),
    modalBackdrop: document.getElementById('modal-backdrop'),
    modalQuickLaunch: document.getElementById('modal-quick-launch'),
    modalAddServer: document.getElementById('modal-add-server'),
    qlSearchInput: document.getElementById('ql-search-input'),
    formAddServer: document.getElementById('form-add-server'),
    btnCloseAddModal: document.getElementById('btn-close-add-modal'),
    btnCancelAdd: document.getElementById('btn-cancel-add'),
    colGroupedServers: document.getElementById('col-grouped-servers'),
    colManagedServers: document.getElementById('col-managed-servers')
  };

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
    const workspace = document.querySelector('.workspace-layout');
    const groupBar = document.querySelector('.group-filter-bar');
    
    if (workspace) workspace.style.display = isServers ? 'flex' : 'none';
    if (groupBar) groupBar.style.display = isServers ? 'flex' : 'none';
    
    ['scripts', 'cmds', 'systemd', 'sensors'].forEach(v => {
      const el = document.getElementById('view-' + v);
      if (el) el.style.display = (state.activeTab === v) ? 'flex' : 'none';
    });
  }

function setupGroupFilter() {
    DOM.groupPills.forEach(pill => {
      pill.addEventListener('click', () => {
        DOM.groupPills.forEach(p => p.classList.remove('active'));
        pill.classList.add('active');
        state.activeGroup = pill.getAttribute('data-group');
        applyGroupFilter();
      });
    });
  }

  function applyGroupFilter() {
    const sections = document.querySelectorAll('.group-section');
    sections.forEach(sec => {
      const group = sec.getAttribute('data-group-section');
      if (state.activeGroup === 'all' || state.activeGroup === group) {
        sec.style.display = 'flex';
      } else {
        sec.style.display = 'none';
      }
    });
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

function toggleServerStatus(serverId) {
    const s = state.servers.find(x => x.id === serverId || serverId.startsWith(x.id));
    if (!s) return;
    
    s.status = s.status === 'running' ? 'stopped' : 'running';
    updateServerCardUI(s);
    updateOverviewStats();
  }

  function updateServerCardUI(server) {
    const cards = document.querySelectorAll(`[id$="${server.id}"]`);
    cards.forEach(card => {
      const dot = card.querySelector('.status-dot');
      const pill = card.querySelector('.status-pill');
      const btn = card.querySelector('.btn-action-start, .btn-action-stop');
      
      if (server.status === 'running') {
        if (dot) { dot.classList.remove('stopped'); dot.classList.add('running'); }
        if (pill) { pill.classList.remove('stopped'); pill.classList.add('running'); pill.textContent = 'Running'; }
        if (btn) {
          btn.className = 'btn-action-stop';
          btn.innerHTML = '<span class="icon">&#xe18c;</span><span>Stop</span>';
        }
      } else {
        if (dot) { dot.classList.remove('running'); dot.classList.add('stopped'); }
        if (pill) { pill.classList.remove('running'); pill.classList.add('stopped'); pill.textContent = 'Stopped'; }
        if (btn) {
          btn.className = 'btn-action-start';
          btn.innerHTML = '<span class="icon">&#xe1c4;</span><span>Start</span>';
        }
      }
    });
  }

function setupBatchActions() {
    document.querySelectorAll('.btn-batch').forEach(btn => {
      btn.addEventListener('click', () => {
        const group = btn.getAttribute('data-batch-group');
        const action = btn.getAttribute('data-action');
        
        state.servers.filter(s => s.group === group).forEach(s => {
          s.status = action === 'start' ? 'running' : 'stopped';
          updateServerCardUI(s);
        });
        updateOverviewStats();
      });
    });
  }

function setupSearch() {
    const handleSearch = (q) => {
      state.searchQuery = q.toLowerCase().trim();
      const cards = document.querySelectorAll('.server-card');
      cards.forEach(card => {
        const text = card.textContent.toLowerCase();
        card.style.display = text.includes(state.searchQuery) ? 'flex' : 'none';
      });
    };

    if (DOM.globalSearch) {
      DOM.globalSearch.addEventListener('input', (e) => handleSearch(e.target.value));
    }
    if (DOM.subbarSearch) {
      DOM.subbarSearch.addEventListener('input', (e) => handleSearch(e.target.value));
    }
  }

function openQuickLaunch() {
    if (!DOM.modalBackdrop || !DOM.modalQuickLaunch) return;
    DOM.modalBackdrop.style.display = 'flex';
    DOM.modalQuickLaunch.style.display = 'block';
    DOM.modalAddServer.style.display = 'none';
    if (DOM.qlSearchInput) {
      DOM.qlSearchInput.value = '';
      DOM.qlSearchInput.focus();
    }
  }

  function closeModal() {
    if (!DOM.modalBackdrop) return;
    DOM.modalBackdrop.style.display = 'none';
    if (DOM.modalQuickLaunch) DOM.modalQuickLaunch.style.display = 'none';
    if (DOM.modalAddServer) DOM.modalAddServer.style.display = 'none';
  }

function openAddServer() {
    if (!DOM.modalBackdrop || !DOM.modalAddServer) return;
    DOM.modalBackdrop.style.display = 'flex';
    DOM.modalAddServer.style.display = 'block';
    DOM.modalQuickLaunch.style.display = 'none';
  }

  function setupAddServerForm() {
    if (DOM.btnAddServer) DOM.btnAddServer.addEventListener('click', openAddServer);
    if (DOM.btnCloseAddModal) DOM.btnCloseAddModal.addEventListener('click', closeModal);
    if (DOM.btnCancelAdd) DOM.btnCancelAdd.addEventListener('click', closeModal);
    
    if (DOM.formAddServer) {
      DOM.formAddServer.addEventListener('submit', (e) => {
        e.preventDefault();
        const fd = new FormData(DOM.formAddServer);
        const name = fd.get('name');
        const cmd = fd.get('command');
        const port = fd.get('port');
        const group = fd.get('group');
        const folder = fd.get('directory') || 'custom';
        
        const id = name.toLowerCase().replace(/[^a-z0-9]/g, '-');
        state.servers.push({ id, name, group, cmd, port, folder, status: 'stopped' });
        
        closeModal();
        DOM.formAddServer.reset();
        alert(`Server "${name}" added successfully.`);
      });
    }
  }

function setupShortcuts() {
    window.addEventListener('keydown', (e) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        if (DOM.globalSearch) DOM.globalSearch.focus();
      } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'p') {
        e.preventDefault();
        openQuickLaunch();
      } else if (e.key === 'Escape') {
        closeModal();
      }
    });
    
    if (DOM.btnQuickLaunch) {
      DOM.btnQuickLaunch.addEventListener('click', openQuickLaunch);
    }
  }

function setupScanTimer() {
    setInterval(() => {
      state.scanElapsedSec++;
      const m = String(Math.floor(state.scanElapsedSec / 60)).padStart(2, '0');
      const s = String(state.scanElapsedSec % 60).padStart(2, '0');
      if (DOM.scanTimer) {
        DOM.scanTimer.textContent = `last scan: 00:${m}:${s}`;
      }
    }, 1000);

    if (DOM.btnRefresh) {
      DOM.btnRefresh.addEventListener('click', () => {
        state.scanElapsedSec = 0;
        DOM.btnRefresh.classList.add('spinning');
        setTimeout(() => DOM.btnRefresh.classList.remove('spinning'), 500);
      });
    }
  }

  function updateOverviewStats() {
    const running = state.servers.filter(s => s.status === 'running').length;
    const total = state.servers.length;
    const badge = document.querySelector('.subtab-badge');
    if (badge) badge.textContent = `${running}/${total}`;
  }

function setupWindowControls() {
    const btnMin = document.getElementById('btn-minimize');
    const btnMax = document.getElementById('btn-maximize');
    const btnClose = document.getElementById('btn-close');

    if (window.__TAURI__ && window.__TAURI__.window) {
      const appWin = window.__TAURI__.window.getCurrentWindow();
      if (btnMin) btnMin.addEventListener('click', () => appWin.minimize());
      if (btnMax) btnMax.addEventListener('click', () => appWin.toggleMaximize());
      if (btnClose) btnClose.addEventListener('click', () => appWin.close());
    }
  }
