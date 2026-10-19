/* 诊断 search_text：不 await，轮询状态 + 抓全部 console，定位卡点 */
import { launch, sleep, freshProfile } from './cdp.mjs';
const URL = process.env.TEST_URL || 'http://127.0.0.1:8123/index.html';
const browser = await launch({ profileDir: freshProfile('diag'), port: 9460 });
const page = await browser.newPage();
const logs = [];
page.on((m) => {
  if (m.method === 'Runtime.consoleAPICalled') {
    logs.push('[' + m.params.type + '] ' + (m.params.args || []).map(a => a.value ?? a.description ?? a.type).join(' '));
  }
  if (m.method === 'Runtime.exceptionThrown') {
    logs.push('[EXC] ' + (m.params.exceptionDetails.exception?.description || m.params.exceptionDetails.text));
  }
});
const E = async (expr) => {
  const r = await page.send('Runtime.evaluate', { expression: expr, returnByValue: true, awaitPromise: true }, 20000);
  if (r.exceptionDetails) throw new Error('eval: ' + (r.exceptionDetails.exception?.description || r.exceptionDetails.text));
  return r.result.value;
};
await page.send('Page.navigate', { url: URL });
await sleep(1500);
let ready = false;
for (let i = 0; i < 50; i++) { if (await E('!!window._wasmReady').catch(() => false)) { ready = true; break; } await sleep(500); }
console.log('READY:', ready);

// 加载 PDF 后触发搜索（不 await）
await E(`(async () => {
  const plugin = window.OFD_Plugin;
  const buf = await (await fetch('test_labor_contract.pdf')).arrayBuffer();
  await plugin.LoadFile(new Uint8Array(buf), 'test_labor_contract.pdf');
  await plugin.SetPageMode(1, 100);
  await new Promise(r => setTimeout(r, 800));
  window.__ss = 'running';
  window.__sr = null; window.__se = null;
  try {
    plugin.SearchText('楚红', 0, 0).then(r => { window.__ss='done'; window.__sr=r; })
      .catch(e => { window.__ss='err'; window.__se=String(e); });
  } catch(e) { window.__ss='sync-err'; window.__se=String(e); }
  return 'fired';
})()`);

let state = 'unknown';
for (let i = 0; i < 60; i++) {
  await sleep(500);
  state = await E('window.__ss').catch(() => 'eval-dead');
  if (state === 'done' || state === 'err' || state === 'sync-err' || state === 'eval-dead') break;
}
const res = await E('JSON.stringify({ss:window.__ss, se:window.__se, sr:window.__sr})').catch(e => 'eval-failed:' + e.message);

console.log('STATE:', state);
console.log('RESULT:', typeof res === 'string' ? res.slice(0, 400) : res);
console.log('--- console (last 30) ---');
console.log(logs.slice(-30).join('\n'));
browser.close();
