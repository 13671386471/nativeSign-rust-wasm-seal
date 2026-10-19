/* 用本项目引擎渲染 opensign 合同第0页并截图 */
import { launch, sleep, freshProfile } from './cdp.mjs';
import { writeFileSync, mkdirSync } from 'node:fs';

mkdirSync('out', { recursive: true });
const URL = process.env.TEST_URL || 'http://127.0.0.1:8123/index.html';

const browser = await launch({ profileDir: freshProfile('probe'), port: 9420 });
const page = await browser.newPage();
const errs = [];
page.on((m) => {
  if (m.method === 'Runtime.consoleAPICalled' && m.params.type === 'error')
    errs.push('ERR: ' + (m.params.args || []).map((a) => a.value ?? a.description ?? a.type).join(' '));
  if (m.method === 'Runtime.exceptionThrown')
    errs.push('EXC: ' + (m.params.exceptionDetails.exception?.description || m.params.exceptionDetails.text));
});

await page.send('Page.navigate', { url: URL });
await sleep(1500);
for (let i = 0; i < 40; i++) { if (await page.eval('!!window._wasmReady').catch(() => false)) break; await sleep(500); }

const res = await page.eval(`(async () => {
  const plugin = window.OFD_Plugin;
  const buf = await (await fetch('_probe_opensign.pdf')).arrayBuffer();
  await plugin.LoadFile(new Uint8Array(buf), 'opensign.pdf');
  const pages = await plugin.GetPageCount();
  await plugin.SetPageMode(1, 100);
  await new Promise(r => setTimeout(r, 2500));
  const canvases = document.querySelectorAll('#screen canvas[data-page]');
  const info = [];
  canvases.forEach(c => info.push({ page: c.getAttribute('data-page'), w: c.width, h: c.height }));
  // 签章信息
  let sealJson = null;
  try { sealJson = await plugin.GetSealInfoJson(); } catch(e){}
  let sealN = 0; try { sealN = (JSON.parse(sealJson||'[]')).length; } catch(e){}
  return JSON.stringify({ pages, canvases: info, sealN });
})()`);

console.log('=== 渲染信息 ===\n' + res);
if (errs.length) console.log('=== 错误 ===\n' + errs.slice(0,10).join('\n'));

const shot = await page.send('Page.captureScreenshot', { format: 'png' });
writeFileSync('out/our-engine-page0.png', Buffer.from(shot.data, 'base64'));
console.log('截图: out/our-engine-page0.png  (测试目录 out/)');
browser.close();
