/* 文本搜索验证：分步 + 触发后轮询（避免长 await 卡死），PDF"楚红" / OFD"电子签名" */
import { launch, sleep, freshProfile } from './cdp.mjs';
import { writeFileSync, mkdirSync } from 'node:fs';
mkdirSync('out', { recursive: true });
const URL = process.env.TEST_URL || 'http://127.0.0.1:8123/index.html';

const browser = await launch({ profileDir: freshProfile('search'), port: 9450 });
const page = await browser.newPage();
page.on((m) => { if (m.method === 'Page.javascriptDialogOpening') page.send('Page.handleJavaScriptDialog', { accept: true }).catch(() => {}); });

const E = (expr, ms = 30000) => page.send('Runtime.evaluate', { expression: expr, returnByValue: true, awaitPromise: true }, ms)
  .then(r => { if (r.exceptionDetails) throw new Error('eval: ' + (r.exceptionDetails.exception?.description || r.exceptionDetails.text)); return r.result.value; });

const poll = async (varName, maxMs = 25000) => {
  const t0 = Date.now();
  while (Date.now() - t0 < maxMs) {
    const s = await E(`window.${varName} || 'running'`, 8000).catch(() => 'eval-dead');
    if (s === 'done' || s === 'err') return s;
    if (s === 'eval-dead') return 'eval-dead';
    await sleep(300);
  }
  return 'timeout';
};

let out = {};
try {
  await page.send('Page.navigate', { url: URL });
  await sleep(1500);
  let ready = false;
  for (let i = 0; i < 60; i++) { if (await E('!!window._wasmReady', 5000).catch(() => false)) { ready = true; break; } await sleep(500); }
  out.ready = ready;

  // 1. 加载 PDF
  await E(`(async()=>{const p=window.OFD_Plugin;const b=await(await fetch('test_labor_contract.pdf')).arrayBuffer();await p.LoadFile(new Uint8Array(b),'test_labor_contract.pdf');await p.SetPageMode(1,100);return 1;})()`, 90000);

  // 2. 直接调用引擎 SearchText（fire + poll）
  await E(`(()=>{window.__ds='running';window.__dr=null;window.__de=null;window.OFD_Plugin.SearchText('楚红',0,0).then(r=>{window.__dr=r;window.__ds='done';}).catch(e=>{window.__de=String(e);window.__ds='err';});return 1;})()`);
  out.directState = await poll('__ds', 30000);
  out.direct = await E('JSON.stringify({r:window.__dr,e:window.__de})', 8000);

  // 3. UI 入口搜索
  await E(`(()=>{document.getElementById('searchInput').value='楚红';document.getElementById('searchBtn').click();return 1;})()`);
  for (let i = 0; i < 60; i++) { out.uiSummary = await E(`document.getElementById('searchSummary').textContent`, 8000).catch(() => ''); if (out.uiSummary && out.uiSummary !== '搜索中…') break; await sleep(200); }
  out.uiPanel = await E(`JSON.stringify({display:getComputedStyle(document.getElementById('searchResults')).display, items:document.getElementById('searchResults').querySelectorAll('.sr-item').length, text:(document.getElementById('searchResults').innerText||'').slice(0,160)})`, 8000);

  const shot = await page.send('Page.captureScreenshot', { format: 'png' });
  writeFileSync('out/search-pdf.png', Buffer.from(shot.data, 'base64'));

  // 4. 未命中
  await E(`(()=>{document.getElementById('searchInput').value='不存在的词XYZQ';document.getElementById('searchBtn').click();return 1;})()`);
  for (let i = 0; i < 60; i++) { out.missSummary = await E(`document.getElementById('searchSummary').textContent`, 8000).catch(() => ''); if (out.missSummary && out.missSummary !== '搜索中…') break; await sleep(200); }

  // 5. OFD
  await E(`(async()=>{const p=window.OFD_Plugin;const b=await(await fetch('sample.ofd')).arrayBuffer();await p.LoadFile(new Uint8Array(b),'sample.ofd');await p.SetPageMode(1,100);return 1;})()`, 90000);
  await E(`(()=>{window.__os='running';window.__or=null;window.__oe=null;window.OFD_Plugin.SearchText('电子签名',0,0).then(r=>{window.__or=r;window.__os='done';}).catch(e=>{window.__oe=String(e);window.__os='err';});return 1;})()`);
  out.ofdState = await poll('__os', 30000);
  out.ofd = await E('JSON.stringify({r:window.__or,e:window.__oe})', 8000);
  await E(`(()=>{document.getElementById('searchInput').value='电子签名';document.getElementById('searchBtn').click();return 1;})()`);
  for (let i = 0; i < 60; i++) { out.ofdSummary = await E(`document.getElementById('searchSummary').textContent`, 8000).catch(() => ''); if (out.ofdSummary && out.ofdSummary !== '搜索中…') break; await sleep(200); }
} catch (e) {
  out.error = String(e && e.message || e);
} finally {
  console.log(JSON.stringify(out, null, 2));
  browser.close();
}
