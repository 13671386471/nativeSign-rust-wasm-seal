/* 单独诊断 OFD：加载 → 渲染 → 搜索，抓 console，定位卡点 */
import { launch, sleep, freshProfile } from './cdp.mjs';
const URL = process.env.TEST_URL || 'http://127.0.0.1:8123/index.html';
const browser = await launch({ profileDir: freshProfile('ofd'), port: 9470 });
const page = await browser.newPage();
const logs = [];
page.on((m) => {
  if (m.method === 'Runtime.consoleAPICalled') logs.push('[' + m.params.type + '] ' + (m.params.args || []).map(a => a.value ?? a.description ?? a.type).join(' '));
  if (m.method === 'Runtime.exceptionThrown') logs.push('[EXC] ' + (m.params.exceptionDetails.exception?.description || m.params.exceptionDetails.text));
  if (m.method === 'Page.javascriptDialogOpening') page.send('Page.handleJavaScriptDialog', { accept: true }).catch(() => {});
});
const E = (expr, ms = 20000) => page.send('Runtime.evaluate', { expression: expr, returnByValue: true, awaitPromise: true }, ms)
  .then(r => { if (r.exceptionDetails) throw new Error('eval: ' + (r.exceptionDetails.exception?.description || r.exceptionDetails.text)); return r.result.value; });

try {
  await page.send('Page.navigate', { url: URL });
  await sleep(1500);
  for (let i = 0; i < 60; i++) { if (await E('!!window._wasmReady', 5000).catch(() => false)) break; await sleep(500); }

  // 加载 OFD（不 await 整段，用状态标记）
  await E(`(async()=>{window.__ls='running';try{const p=window.OFD_Plugin;const b=await(await fetch('sample.ofd')).arrayBuffer();await p.LoadFile(new Uint8Array(b),'sample.ofd');window.__ls='loaded';await p.SetPageMode(1,100);window.__ls='rendered';}catch(e){window.__ls='err';window.__le=String(e);}return 1;})()`, 60000).catch(e => { console.log('LOAD eval err:', e.message); });
  let ls = 'unknown';
  for (let i = 0; i < 40; i++) { ls = await E('window.__ls', 5000).catch(() => 'eval-dead'); if (ls === 'rendered' || ls === 'err' || ls === 'eval-dead') break; await sleep(500); }
  console.log('LOAD STATE:', ls);

  // 触发搜索
  await E(`(()=>{window.__os='running';window.OFD_Plugin.SearchText('电子签名',0,0).then(r=>{window.__or=r;window.__os='done';}).catch(e=>{window.__oe=String(e);window.__os='err';});return 1;})()`, 15000).catch(e => console.log('FIRE eval err:', e.message));
  let os = 'unknown';
  for (let i = 0; i < 40; i++) { os = await E('window.__os', 5000).catch(() => 'eval-dead'); if (os !== 'running' && os !== 'unknown') break; await sleep(400); }
  const res = await E('JSON.stringify({os:window.__os,oe:window.__oe,or:window.__or})', 8000).catch(e => 'eval-failed:' + e.message);
  console.log('SEARCH STATE:', os);
  console.log('SEARCH RESULT:', typeof res === 'string' ? res.slice(0, 300) : res);
  console.log('--- console (last 25) ---');
  console.log(logs.slice(-25).join('\n'));
} finally {
  browser.close();
}
