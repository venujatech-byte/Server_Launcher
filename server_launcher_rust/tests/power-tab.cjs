const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

const app = fs.readFileSync('web/app.js', 'utf8');
const power = app.slice(app.indexOf('      power: () => {'), app.indexOf('      bluetooth:', app.indexOf('      power: () => {'))).trim().replace(/,$/, '');
function render(overrides = {}) {
  return vm.runInNewContext(`({${power}}).power()`, {
    s: { battery_pct: 50, battery_status: 'Discharging', battery_voltage_mv: 4000,
      power_attribution_secs: 3600, power_attribution: [{ label: 'CPU', mah: 1000 }], ...overrides },
    state: {}, hist: [], battHist: [50], tempHist: [30], uptimeHours: 1, uptimeMins: 0,
    val: value => value ?? '—', avg: () => 0, peak: () => 0,
    escapeHtml: value => value, empty: title => title, icon: () => '',
    renderBigStatCard: card => JSON.stringify(card), renderWaveformChartSvg: () => '',
  });
}
assert.match(render(), /4\.000 W/);
assert.match(render(), /estimated average|Estimated average/);
for (const overrides of [{ power_attribution_secs: 0 }, { power_attribution_secs: null }, { battery_voltage_mv: null }]) {
  assert.match(render(overrides), /Average watts unavailable/);
  assert.doesNotMatch(render(overrides), /NaN|Infinity/);
}
assert.match(render({ power_attribution: [] }), /No power attribution history/);
assert.match(render({ power_attribution: [{ label: 'CPU', mah: 0 }] }), /0\.000 W/);
console.log('Power tab rendering and watts conversion checks passed.');
