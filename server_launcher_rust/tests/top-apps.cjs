const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const app = fs.readFileSync('web/app.js', 'utf8');
const start = app.indexOf('    function isForegroundApp(');
const source = app.slice(start, app.indexOf('    // 11 Subtab Views', start));
function render(processes = [], changes = {}) {
  return vm.runInNewContext(`${source}\ntopAppsTable()`, {
    processes, s: { cpu_cores: 8, cpu_usage_pct: 44 }, memUsage: 60, Set,
    state: { sensorHistory: [], sensorsCadence: 5000, topAppsExpandedGroups: new Set(), ...changes },
    escapeHtml: value => String(value).replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('"', '&quot;'),
  });
}
const rows = [{ pid: 10, name: 'com.example.notes', memory_bytes: 250 * 1024 ** 2 }, { pid: 11, name: 'com.example.notes:worker', memory_bytes: 50 * 1024 ** 2 }];
assert.match(render(rows), /2 processes/);
assert.match(render(rows), /300.0 MB/);
assert.match(render(rows, { topAppsExpandedGroups: new Set(['com.example.notes']) }), /PID 11/);
assert.match(render(rows, { topAppsSearch: 'missing' }), /No processes match/);
assert.match(render(), /0 processes/);
assert.match(render(), /No process data available/);
assert.doesNotMatch(render(), /antigravity|data-pid=/);
assert.ok(render(rows).indexOf('tm-refresh-cadence') < render(rows).indexOf('sensor-app-filter'));
assert.ok(render(rows).indexOf('sensor-app-filter') < render(rows).indexOf('tm-btn-end-task'));
module.exports = { render };
console.log('Top Apps grouping, expansion, search, empty-state and toolbar checks passed.');
