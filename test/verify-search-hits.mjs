/* 验证"甲方"整词框选 + 关闭清除标记（使用用户实际 PDF） */
import { launch, sleep, freshProfile } from './cdp.mjs';
import { writeFileSync, mkdirSync } from 'node:fs';
mkdirSync('out', { recursive: true });
const URL = process.env.TEST_URL || 'http://127.0.0.1:8123/index.html';

const browser = await launch({ profileDir: freshProfile('hits'), port: 9500 });
const page = await browser.newPage();
page.on((m) => { if (m.method === 'Page.javascriptDialogOpening') page.send('Page.handleJavaScriptDialog', { accept: true }).catch(() => {}); });
const E = (expr, ms = 90000) => page.send('Runtime.evaluate', { expression: expr, returnByValue: true, awaitPromise: true }, ms)
  .then(r => { if (r.exceptionDetails) throw new Error('eval: ' + (r.exceptionDetails.exception?.description || r.exceptionDetails.text)); return r.result.value; });

const ORANGE_BBOX = `(() => {
  const c = document.querySelector('#screen canvas[data-page="0"]'); if (!c) return { err: 'no canvas' };
  const ctx = c.getContext('2d'); const W = c.width, H = c.height;
  let img; try { img = ctx.getImageData(0, 0, W, H).data; } catch (e) { return { err: String(e) }; }
  let n = 0, minX = 1e9, maxX = -1, minY = 1e9, maxY = -1;
  for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) {
    const i = (y * W + x) * 4; const r = img[i], g = img[i+1], b = img[i+2], a = img[i+3];
    if (a > 0 && r > 240 && g >= 185 && g <= 234 && b >= 105 && b <= 185) {
      n++; if (x < minX) minX = x; if (x > maxX) maxX = x; if (y < minY) minY = y; if (y > maxY) maxY = y;
    }
  }
  return n ? { n, w: maxX - minX + 1, h: maxY - minY + 1, minX, minY } : { n: 0 };
})()`;

const COUNT_HL = `(() => {
  const c = document.querySelector('#screen canvas[data-page="0"]'); if (!c) return -1;
  const ctx = c.getContext('2d'); const W = c.width, H = c.height;
  let img; try { img = ctx.getImageData(0, 0, W, H).data; } catch (e) { return -2; }
  let n = 0;
  for (let i = 0; i < img.length; i += 4) {
    const r = img[i], g = img[i+1], b = img[i+2], a = img[i+3];
    if (a > 0 && r > 235 && g > 180 && b < 200 && (r - b) > 30) n++;
  }
  return n;
})()`;

try {
  await page.send('Page.navigate', { url: URL });
  await sleep(1500);
  for (let i = 0; i < 60; i++) { if (await E('!!window._wasmReady', 5000).catch(() => false)) break; await sleep(500); }

  await E(`(async () => {
    const p = window.OFD_Plugin;
    const b = await (await fetch('_probe_form.pdf')).arrayBuffer();
    await p.LoadFile(new Uint8Array(b), '1.劳动合同（2份）-3.26.pdf');
    await p.SetPageMode(1, 100);
    return 1;
  })()`);
  await sleep(500);

  const hlBefore = await E(COUNT_HL);

  // 打开 Rust 查找弹框
  await E(`(() => { document.querySelector('.query-btn').click(); return 1; })()`);
  await sleep(250);

  // 输入"甲方"，先点"下一个"（只高亮当前命中，便于测量单个命中宽度）
  await E(`(() => { document.getElementById('__search_input').value = '甲方'; document.getElementById('__search_next').click(); return 1; })()`);
  await sleep(700);
  const statusNext1 = await E(`document.getElementById('__search_status').textContent`);
  const bbox = await E(ORANGE_BBOX);

  // 全部标记
  await E(`(() => { document.getElementById('__search_all').click(); return 1; })()`);
  await sleep(600);
  const statusAll = await E(`document.getElementById('__search_status').textContent`);
  const hlAll = await E(COUNT_HL);

  const shot = await page.send('Page.captureScreenshot', { format: 'png' });
  writeFileSync('out/search-hits.png', Buffer.from(shot.data, 'base64'));

  // 关闭 → 标记应全部清除
  await E(`(() => { document.getElementById('__search_close').click(); return 1; })()`);
  await sleep(700);
  const modalGone = await E(`!document.getElementById('__search_modal')`);
  const hlAfterClose = await E(COUNT_HL);

  console.log(JSON.stringify({
    hlBefore, statusNext1, currentHitBBox: bbox, statusAll, hlAll,
    modalGone, hlAfterClose
  }, null, 2));
} catch (e) {
  console.log('ERR:', String(e && e.message || e));
} finally {
  browser.close();
}
