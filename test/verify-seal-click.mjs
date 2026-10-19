/* E2E: 加载 PDF → 落章 → 渲染 → 左键点章中心 → 红框可见时截图 → 点空白隐藏 */
import { launch, sleep, freshProfile } from './cdp.mjs';
import { writeFileSync, mkdirSync } from 'node:fs';

mkdirSync('out', { recursive: true });
const URL = process.env.TEST_URL || 'http://127.0.0.1:8123/index.html';

const browser = await launch({ profileDir: freshProfile('seal'), port: 9410 });
const page = await browser.newPage();
const errs = [];
page.on((m) => {
  if (m.method === 'Runtime.exceptionThrown') {
    errs.push('EXC: ' + (m.params.exceptionDetails.exception?.description || m.params.exceptionDetails.text));
  }
  if (m.method === 'Runtime.consoleAPICalled' && m.params.type === 'error') {
    errs.push('ERR: ' + (m.params.args || []).map((a) => a.value ?? a.description ?? a.type).join(' '));
  }
});

await page.send('Page.navigate', { url: URL });
await sleep(1500);
for (let i = 0; i < 40; i++) {
  const ready = await page.eval('!!window._wasmReady').catch(() => false);
  if (ready) break;
  await sleep(500);
}

// A. 加载 + 落章 + 渲染，返回 canvas 几何与章中心点击坐标
const setup = await page.eval(`(async () => {
  const plugin = window.OFD_Plugin;
  const buf = await (await fetch('test_labor_contract.pdf')).arrayBuffer();
  await plugin.LoadFile(new Uint8Array(buf), 'test_labor_contract.pdf');
  const pages = await plugin.GetPageCount();
  const b64 = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';
  plugin.SetCurrentSealInfo('测试章', b64, '测试人', 'SM2');
  const sealCount = await plugin.AddSeal('0,5000,0,0,5000', '', '');
  await plugin.SetPageMode(1, 100);
  await new Promise(r => setTimeout(r, 1800));
  const canvas = document.querySelector('#screen canvas[data-page="0"]');
  if (!canvas) return JSON.stringify({ ok:false, reason:'no canvas' });
  const rect = canvas.getBoundingClientRect();
  let pageW = Number(await plugin.GetPageWidth(0)) || 595;
  let pageH = Number(await plugin.GetPageHeight(0)) || 842;
  const clickX = (59.5 + 60) * (rect.width / pageW);
  const clickY = (84.2 + 60) * (rect.height / pageH);
  const hit = plugin.HitTestSeal(0, rect.width, rect.height, clickX, clickY);
  window.__t = { canvas, rect, clickX, clickY };
  return JSON.stringify({ ok:true, pages, sealCount, pageW, pageH,
    rect: { rw: rect.width, rh: rect.height }, clickX: Math.round(clickX), clickY: Math.round(clickY), hit });
})()`);

// B. 派发左键 click → 红框显示
const boxShown = await page.eval(`(async () => {
  const { canvas, rect, clickX, clickY } = window.__t;
  canvas.dispatchEvent(new MouseEvent('click', { bubbles:true, cancelable:true, button:0,
    clientX: rect.left + clickX, clientY: rect.top + clickY }));
  await new Promise(r => setTimeout(r, 150));
  const box = document.getElementById('sealSelectionBox');
  return JSON.stringify({ display: getComputedStyle(box).display,
    left: box.style.left, top: box.style.top, width: box.style.width, height: box.style.height,
    offsetW: box.offsetWidth, offsetH: box.offsetHeight });
})()`);

// C. 红框可见时截图
const shot = await page.send('Page.captureScreenshot', { format: 'png' });
writeFileSync('out/seal-box.png', Buffer.from(shot.data, 'base64'));

// D. 点空白 → 应隐藏
const boxHidden = await page.eval(`(async () => {
  const { canvas, rect } = window.__t;
  canvas.dispatchEvent(new MouseEvent('click', { bubbles:true, cancelable:true, button:0,
    clientX: rect.left + 2, clientY: rect.top + 2 }));
  await new Promise(r => setTimeout(r, 80));
  return getComputedStyle(document.getElementById('sealSelectionBox')).display;
})()`);

console.log('=== setup ===\n' + setup);
console.log('=== 点击章后红框 ===\n' + boxShown);
console.log('=== 点空白后红框 ===\n' + boxHidden);
if (errs.length) console.log('=== 页面错误 ===\n' + errs.join('\n'));
console.log('截图(红框可见): out/seal-box.png');
browser.close();
