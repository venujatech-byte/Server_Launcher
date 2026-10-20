const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

const app = fs.readFileSync('web/app.js', 'utf8');
const power = app.slice(app.indexOf('      power: () => {'), app.indexOf('      bluetooth:', app.indexOf('      power: () => {'))).trim().replace(/,$/, '');
function render(overrides = {}, history = []) {
  return vm.runInNewContext(`({${power}}).power()`, {
    s: { battery_pct: 50, battery_status: 'Discharging', battery_voltage_mv: 4000,
      power_attribution_secs: 3600, power_attribution: [{ label: 'CPU', mah: 1000 }], ...overrides },
    state: {}, hist: history, battHist: [50], tempHist: [30], uptimeHours: 1, uptimeMins: 0,
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
const plugged = render({ battery_plugged: 'USB', battery_current_ua: -500000 });
assert.match(plugged, /HISTORY · PLUGGED IN/);
assert.match(plugged, /2\.000 W/);
assert.match(plugged, /Not reported separately: GPU/);
assert.match(render({ battery_current_ua: -250000 }), /1\.000 W/);
assert.match(render({ battery_current_ua: 0 }), /0\.000 W/);
const previous = { powerAttributionSecs: 3590, powerAttribution: [{ label: 'CPU', mah: 999 }] };
const interval = render({}, [previous, {}]);
assert.match(interval, /INTERVAL · AVG W/);
assert.match(interval, /1\.440 W/);
assert.match(interval, /10\.000 seconds between samples/);
assert.match(render({ power_attribution_secs: 1 }, [previous, {}]), /HISTORY · AVG W/);
assert.match(render({ power_attribution: [{ label: 'CPU', mah: 1 }] }, [previous, {}]), /HISTORY · AVG W/);
assert.match(render({ battery_plugged: 'USB' }, [previous, {}]), /HISTORY · PLUGGED IN/);
assert.match(plugged, /aria-describedby="power-help-text"/);
assert.match(plugged, /role="tooltip"/);
assert.doesNotMatch(plugged, /<p[^>]*>Plugged in:/);
assert.doesNotMatch(plugged, /<p[^>]*>Not reported separately:/);
module.exports = { render };
console.log('Power tab rendering, live readings, interval/reset and watts conversion checks passed.');
