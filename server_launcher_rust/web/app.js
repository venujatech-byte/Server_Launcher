
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
