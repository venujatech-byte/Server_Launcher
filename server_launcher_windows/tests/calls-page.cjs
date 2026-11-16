const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const app = fs.readFileSync('web/app.js', 'utf8');
const source = app.slice(app.indexOf('  async function renderCalls()'), app.indexOf('  function renderPhotos()', app.indexOf('  async function renderCalls()')));
async function run(state, load) {
  const content = { innerHTML: '' };
  const elements = new Map();
  const context = { state, content, invokeOrFail: load, icon: () => '', escapeHtml: String,
    empty: (title, detail) => `${title}: ${detail}`, toast: () => {}, action: async () => {},
    document: { querySelectorAll: () => [] },
    $: id => { if (!elements.has(id)) elements.set(id, { value: '12345' }); return elements.get(id); },
  };
  vm.createContext(context);
  await vm.runInContext(`${source}\nrenderCalls()`, context);
  return { html: content.innerHTML, context, elements };
}
(async () => {
  const state = { serial: 'phone-a', tab: 'calls', data: [] };
  const result = await run(state, async () => [{ name: 'Test Contact', number: '12345' }]);
  assert.match(result.html, /id="contact-list"/);
  assert.match(result.html, /Test Contact/);
  assert.match(result.html, /data-action="call" data-value="12345"/);
  state.serial = 'phone-b';
  const denied = await run(state, async () => { throw new Error('Permission denied'); });
  assert.match(denied.html, /Contacts unavailable/);
  assert.doesNotMatch(denied.html, /Test Contact/);
  state.contacts = null;
  const stale = await run(state, async () => { state.serial = 'phone-c'; return [{ name: 'Stale', number: '1' }]; });
  assert.equal(stale.html, '');
  let resolveCall;
  result.context.action = () => new Promise(resolve => { resolveCall = resolve; });
  const button = result.elements.get('#dial-call');
  const pending = button.onclick();
  assert.equal(button.disabled, true);
  resolveCall();
  await pending;
  assert.equal(button.disabled, false);
  console.log('Calls contact loading, device changes, denied access and pending-call checks passed.');
})().catch(error => { console.error(error); process.exitCode = 1; });
