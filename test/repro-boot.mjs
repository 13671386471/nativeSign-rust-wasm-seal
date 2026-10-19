/* 复现启动期 PDFium 绑定失败，抓取完整控制台（含 pdfium-render 的 log::error 详细原因） */
import { launch, sleep, freshProfile } from './cdp.mjs';
import { writeFileSync, mkdirSync } from 'node:fs';

mkdirSync('out', { recursive: true });
const URL = process.env.TEST_URL || 'http://127.0.0.1:8123/index.html';

const browser = await launch({ profileDir: freshProfile('boot'), port: 9400 });
const page = await browser.newPage();

const lines = [];
page.on((msg) => {
  if (msg.method === 'Runtime.consoleAPICalled') {
    const parts = (msg.params.args || []).map((a) =>
      a.value !== undefined ? String(a.value) : (a.description || a.type));
    lines.push(`[${msg.params.type}] ` + parts.join(' '));
  }
  if (msg.method === 'Runtime.exceptionThrown') {
    const d = msg.params.exceptionDetails;
    lines.push('[EXCEPTION] ' + (d.exception?.description || d.text));
  }
});

await page.send('Page.navigate', { url: URL });

const start = Date.now();
let ready = false, lastStatus = '';
while (Date.now() - start < 25000) {
  try {
    const st = await page.eval(`JSON.stringify({
      ready: !!window._wasmReady,
      status: (document.getElementById('status')||{}).textContent || '',
      info: (document.getElementById('infoStatus')||{}).textContent || ''
    })`);
    const o = JSON.parse(st);
    lastStatus = o.status + ' | ' + o.info;
    if (o.ready) { ready = true; break; }
  } catch (_) { /* 页面还没就绪 */ }
  await sleep(500);
}

await sleep(3000); // 让异步 panic 全部浮出

writeFileSync('out/boot-console.log', lines.join('\n'), 'utf8');

const interesting = lines.filter((l) =>
  /pdfium|绑定|panic|unreachable|error|错误|异常|exception|missing|not defined|wasm/i.test(l));
console.log('=== READY:', ready, '| status:', lastStatus, '===');
console.log('=== 关键日志（' + interesting.length + ' 条）===');
console.log(interesting.join('\n'));

const shot = await page.send('Page.captureScreenshot', { format: 'png' });
writeFileSync('out/boot.png', Buffer.from(shot.data, 'base64'));
console.log('\n截图: out/boot.png');
console.log('完整控制台: out/boot-console.log');

browser.close();
