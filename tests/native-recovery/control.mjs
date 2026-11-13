import assert from 'node:assert/strict';

const [platform, scenario, phase, mode] = process.argv.slice(2);
assert.ok(['ios', 'android'].includes(platform), 'platform: ios or android');
assert.match(scenario, /^[a-z0-9-]{1,64}$/, 'use a fresh scenario for each prepare');
assert.ok(['prepare', 'resume', 'restore'].includes(phase), 'phase: prepare, resume or restore');
assert.ok(mode === undefined || ['reject', 'commit-drop'].includes(mode), 'mode: reject or commit-drop');
const base = `http://127.0.0.1:${process.env.FIXTURE_PORT ?? 18557}`;
async function configure(name, body) {
  const response = await fetch(`${base}/${name}/control`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body),
  });
  assert.equal(response.status, 200, await response.text());
}
if (mode) await configure(scenario, { mode });
await configure(`${platform}-driver`, { scenario, phase });
console.log(JSON.stringify({ platform, scenario, phase, evidence: `${base}/${scenario}/control` }));
