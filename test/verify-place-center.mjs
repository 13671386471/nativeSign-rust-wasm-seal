/* 验证落章以点击点为中心：AddSeal 归一化点 (25000,25000) → 印章中心应为页面中心 */
import { launch, sleep, freshProfile } from './cdp.mjs';
const URL = process.env.TEST_URL || 'http://127.0.0.1:8123/index.html';

const browser = await launch({ profileDir: freshProfile('place'), port: 9510 });
const page = await browser.newPage();
page.on((m) => { if (m.method === 'Page.javascriptDialogOpening') page.send('Page.handleJavaScriptDialog', { accept: true }).catch(() => {}); });
const E = (expr, ms = 90000) => page.send('Runtime.evaluate', { expression: expr, returnByValue: true, awaitPromise: true }, ms)
  .then(r => { if (r.exceptionDetails) throw new Error('eval: ' + (r.exceptionDetails.exception?.description || r.exceptionDetails.text)); return r.result.value; });

try {
  await page.send('Page.navigate', { url: URL });
  await sleep(1500);
  for (let i = 0; i < 60; i++) { if (await E('!!window._wasmReady', 5000).catch(() => false)) break; await sleep(500); }

  const out = await E(`(async () => {
    const p = window.OFD_Plugin;
    const b = await (await fetch('test_labor_contract.pdf')).arrayBuffer();
    await p.LoadFile(new Uint8Array(b), 'test_labor_contract.pdf');
    const b64 = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';
    p.SetCurrentSealInfo('测试章', b64, '测试人', 'SM2');
    await p.AddSeal('0,25000,0,0,25000', '', '');   // 中心 (297.5, 421)
    await p.AddSeal('0,0,0,0,0', '', '');           // 左上角点击 → 夹到 (0,0)
    await p.AddSeal('0,50000,0,0,50000', '', '');   // 右下角点击 → 夹到 (475,722)
    const arr = JSON.parse(await p.GetSealInfoJson());
    return JSON.stringify(arr.map(s => ({ x: s.x, y: s.y, w: s.width, h: s.height,
      cx: s.x + s.width/2, cy: s.y + s.height/2 })));
  })()`);

  console.log(out);
} catch (e) {
  console.log('ERR:', String(e && e.message || e));
} finally {
  browser.close();
}
