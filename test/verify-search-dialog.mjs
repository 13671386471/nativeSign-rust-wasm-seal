/* 验证 Rust 层"查找"弹框：查询按钮 → 弹框(输入+三按钮) → 全部标记 → 下一个 */
import { launch, sleep, freshProfile } from './cdp.mjs';
import { writeFileSync, mkdirSync } from 'node:fs';
mkdirSync('out', { recursive: true });
const URL = process.env.TEST_URL || 'http://127.0.0.1:8123/index.html';

const browser = await launch({ profileDir: freshProfile('sdlg'), port: 9490 });
const page = await browser.newPage();
page.on((m) => { if (m.method === 'Page.javascriptDialogOpening') page.send('Page.handleJavaScriptDialog', { accept: true }).catch(() => {}); });
const E = (expr, ms = 90000) => page.send('Runtime.evaluate', { expression: expr, returnByValue: true, awaitPromise: true }, ms)
  .then(r => { if (r.exceptionDetails) throw new Error('eval: ' + (r.exceptionDetails.exception?.description || r.exceptionDetails.text)); return r.result.value; });

const COUNT_YELLOW = `(() => {
  const c = document.querySelector('#screen canvas[data-page="0"]');
  if (!c) return -1;
  const ctx = c.getContext('2d'); const W = c.width, H = c.height;
  let img; try { img = ctx.getImageData(0, 0, W, H).data; } catch (e) { return -2; }
  let n = 0;
  for (let i = 0; i < img.length; i += 4) {
    const r = img[i], g = img[i+1], b = img[i+2], a = img[i+3];
    if (a > 0 && r > 200 && g > 180 && b < 200 && (r - b) > 40) n++;
  }
  return n;
})()`;

try {
  await page.send('Page.navigate', { url: URL });
  await sleep(1500);
  for (let i = 0; i < 60; i++) { if (await E('!!window._wasmReady', 5000).catch(() => false)) break; await sleep(500); }

  // 加载 PDF（通过 API，模拟已打开文档）
  await E(`(async () => {
    const p = window.OFD_Plugin;
    const b = await (await fetch('test_labor_contract.pdf')).arrayBuffer();
    await p.LoadFile(new Uint8Array(b), 'test_labor_contract.pdf');
    await p.SetPageMode(1, 100);
    return 1;
  })()`);
  await sleep(500);

  const yellowBefore = await E(COUNT_YELLOW);

  // 点击前端"查询"按钮（唯一的前端职责）
  await E(`(() => { const b = document.querySelector('.query-btn'); if (!b) return 'no-btn'; b.click(); return 'clicked'; })()`);
  await sleep(300);

  const modal = await E(`JSON.stringify({
    modal: !!document.getElementById('__search_modal'),
    input: !!document.getElementById('__search_input'),
    all: !!document.getElementById('__search_all'),
    prev: !!document.getElementById('__search_prev'),
    next: !!document.getElementById('__search_next'),
    panelStyle: (document.querySelector('#__search_modal > div')||{}).getAttribute ? document.querySelector('#__search_modal > div').getAttribute('style') : ''
  })`);

  // 输入"劳动" → 全部标记
  await E(`(() => { document.getElementById('__search_input').value = '劳动'; document.getElementById('__search_all').click(); return 1; })()`);
  await sleep(700);
  const statusAll = await E(`document.getElementById('__search_status').textContent`);
  const yellowAfterAll = await E(COUNT_YELLOW);

  const shot = await page.send('Page.captureScreenshot', { format: 'png' });
  writeFileSync('out/search-dialog.png', Buffer.from(shot.data, 'base64'));

  // 下一个
  await E(`(() => { document.getElementById('__search_next').click(); return 1; })()`);
  await sleep(500);
  const statusNext = await E(`document.getElementById('__search_status').textContent`);
  const yellowAfterNext = await E(COUNT_YELLOW);

  // 关闭
  await E(`(() => { document.getElementById('__search_close').click(); return 1; })()`);
  await sleep(200);
  const modalAfterClose = await E(`!!document.getElementById('__search_modal')`);

  console.log(JSON.stringify({
    yellowBefore, modal: JSON.parse(modal),
    statusAll, yellowAfterAll,
    statusNext, yellowAfterNext,
    modalAfterClose
  }, null, 2));
} catch (e) {
  console.log('ERR:', String(e && e.message || e));
} finally {
  browser.close();
}
