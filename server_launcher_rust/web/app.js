
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
