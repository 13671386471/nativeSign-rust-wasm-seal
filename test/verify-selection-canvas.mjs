/* 验证方案B: 选中红框由渲染层画进 canvas（像素层），前端无 DOM 覆盖层 */
import { launch, sleep, freshProfile } from './cdp.mjs';
import { writeFileSync, mkdirSync } from 'node:fs';
mkdirSync('out', { recursive: true });
const URL = process.env.TEST_URL || 'http://127.0.0.1:8123/index.html';

const browser = await launch({ profileDir: freshProfile('selcanvas'), port: 9480 });
const page = await browser.newPage();
page.on((m) => { if (m.method === 'Page.javascriptDialogOpening') page.send('Page.handleJavaScriptDialog', { accept: true }).catch(() => {}); });
const E = (expr, ms = 90000) => page.send('Runtime.evaluate', { expression: expr, returnByValue: true, awaitPromise: true }, ms)
  .then(r => { if (r.exceptionDetails) throw new Error('eval: ' + (r.exceptionDetails.exception?.description || r.exceptionDetails.text)); return r.result.value; });

const COUNT_RED = `(() => {
  const c = document.querySelector('#screen canvas[data-page="0"]');
  if (!c) return { err: 'no canvas' };
  const ctx = c.getContext('2d');
  const W = c.width, H = c.height;
  const sx = W / 595.3, sy = H / 841.9;
  const x0 = Math.max(0, Math.floor(59.5 * sx) - 5), y0 = Math.max(0, Math.floor(84.2 * sy) - 5);
  const w = Math.ceil(120 * sx) + 10, h = Math.ceil(120 * sy) + 10;
  const x1 = Math.min(W, x0 + w), y1 = Math.min(H, y0 + h);
  let img;
  try { img = ctx.getImageData(x0, y0, x1 - x0, y1 - y0).data; } catch (e) { return { err: String(e) }; }
  let n = 0;
  for (let i = 0; i < img.length; i += 4) {
    const r = img[i], g = img[i + 1], b = img[i + 2], a = img[i + 3];
    if (a > 0 && r > 170 && g < 90 && b < 90) n++;
  }
  return { red: n, cw: W, ch: H };
})()`;

try {
  await page.send('Page.navigate', { url: URL });
  await sleep(1500);
  for (let i = 0; i < 60; i++) { if (await E('!!window._wasmReady', 5000).catch(() => false)) break; await sleep(500); }

  // 加载 PDF + 放一枚"透明"章(1x1 透明 PNG), 使红框成为唯一红色来源
  await E(`(async () => {
    const p = window.OFD_Plugin;
    const b = await (await fetch('test_labor_contract.pdf')).arrayBuffer();
    await p.LoadFile(new Uint8Array(b), 'test_labor_contract.pdf');
    const b64 = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';
    p.SetCurrentSealInfo('测试章', b64, '测试人', 'SM2');
    await p.AddSeal('0,5000,0,0,5000', '', '');
    await p.SetPageMode(1, 100);
    return 1;
  })()`);
  await sleep(600);

  const domRemoved = await E(`document.getElementById('sealSelectionBox') === null`);
  const before = await E(COUNT_RED);

  // 左键点击章中心
  const click = await E(`(() => {
    const c = document.querySelector('#screen canvas[data-page="0"]');
    const r = c.getBoundingClientRect();
    const cx = (59.5 + 60) * (r.width / 595.3), cy = (84.2 + 60) * (r.height / 841.9);
    c.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, button: 0, clientX: r.left + cx, clientY: r.top + cy }));
    return JSON.stringify({ cx: Math.round(cx), cy: Math.round(cy) });
  })()`);
  await sleep(500);
  const afterClick = await E(COUNT_RED);

  const shot = await page.send('Page.captureScreenshot', { format: 'png' });
  writeFileSync('out/selection-canvas.png', Buffer.from(shot.data, 'base64'));

  // 点击空白 → 清除选中
  await E(`(() => {
    const c = document.querySelector('#screen canvas[data-page="0"]');
    const r = c.getBoundingClientRect();
    c.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, button: 0, clientX: r.left + 2, clientY: r.top + 2 }));
    return 1;
  })()`);
  await sleep(500);
  const afterEmpty = await E(COUNT_RED);

  const directHit = await E(`(function(){ try { return window.OFD_Plugin.SelectSealAt(0, 595, 841, 119, 144); } catch(e){ return 'ERR:'+e.message; } })()`);

  console.log(JSON.stringify({
    domOverlayRemoved: domRemoved,
    redBeforeClick: before,
    redAfterClick: afterClick,
    redAfterEmptyClick: afterEmpty,
    click, directHit: typeof directHit === 'string' ? directHit.slice(0, 120) : directHit
  }, null, 2));
} catch (e) {
  console.log('ERR:', String(e && e.message || e));
} finally {
  browser.close();
}
