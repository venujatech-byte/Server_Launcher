
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
