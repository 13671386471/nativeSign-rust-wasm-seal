/* 导出引擎预处理后的 PDF（验证 preprocess_pdf_for_cjk 是否污染注释外观） */
import { launch, sleep, freshProfile } from './cdp.mjs';
import { writeFileSync, mkdirSync } from 'node:fs';

mkdirSync('out', { recursive: true });
const URL = process.env.TEST_URL || 'http://127.0.0.1:8123/index.html';
const browser = await launch({ profileDir: freshProfile('dump'), port: 9430 });
const page = await browser.newPage();
await page.send('Page.navigate', { url: URL });
await sleep(1500);
for (let i = 0; i < 40; i++) { if (await page.eval('!!window._wasmReady').catch(() => false)) break; await sleep(500); }

// 1) 原始大小
const orig = await page.eval(`(async () => {
  const r = await fetch('_probe_opensign.pdf'); const b = await r.arrayBuffer(); return b.byteLength;
})()`);
console.log('原始 PDF 大小:', orig);

// 2) 预处理的文档数据长度
const info = await page.eval(`(async () => {
  const plugin = window.OFD_Plugin;
  const buf = await (await fetch('_probe_opensign.pdf')).arrayBuffer();
  await plugin.LoadFile(new Uint8Array(buf), 'opensign.pdf');
  let len = -1, head = '';
  try {
    const d = await plugin.GetDocumentData();
    len = d ? d.length : -1;
    head = d && d.length ? String.fromCharCode.apply(null, d.slice(0,8)) : '';
  } catch(e) { head = 'ERR:'+e.message; }
  return JSON.stringify({ len, head });
})()`);
console.log('GetDocumentData:', info);

// 3) 若不太大，导出为 base64 以便本地 PyMuPDF 渲染
const b64 = await page.eval(`(async () => {
  try {
    const plugin = window.OFD_Plugin;
    const d = await plugin.GetDocumentData();
    if (!d || !d.length) return '';
    let s = '';
    const CH = 0x8000;
    for (let i = 0; i < d.length; i += CH) s += String.fromCharCode.apply(null, d.subarray(i, i+CH));
    return btoa(s);
  } catch(e) { return 'ERR:'+e.message; }
})()`);
if (b64 && !b64.startsWith('ERR')) {
  writeFileSync('out/preprocessed.b64', b64);
  console.log('已写出 out/preprocessed.b64, base64 长度:', b64.length);
} else {
  console.log('导出失败:', b64);
}
browser.close();
