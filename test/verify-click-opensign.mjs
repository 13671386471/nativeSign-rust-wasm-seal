/* 在真实 OpenSign 已签 PDF 上验证: 点已有印章→出现签名域红框; 点空白→隐藏 */
import { launch, sleep, freshProfile } from './cdp.mjs';
import { writeFileSync, mkdirSync } from 'node:fs';
mkdirSync('out', { recursive: true });
const URL = process.env.TEST_URL || 'http://127.0.0.1:8123/index.html';
const browser = await launch({ profileDir: freshProfile('oc'), port: 9440 });
const page = await browser.newPage();
const errs = [];
page.on((m) => {
  if (m.method === 'Runtime.exceptionThrown') errs.push('EXC:' + (m.params.exceptionDetails.exception?.description || m.params.exceptionDetails.text));
  if (m.method === 'Runtime.consoleAPICalled' && m.params.type === 'error') errs.push('ERR:' + (m.params.args||[]).map(a=>a.value??a.description??a.type).join(' '));
});
await page.send('Page.navigate', { url: URL });
await sleep(1500);
for (let i = 0; i < 40; i++) { if (await page.eval('!!window._wasmReady').catch(()=>false)) break; await sleep(500); }

const out = await page.eval(`(async () => {
  const plugin = window.OFD_Plugin;
  const buf = await (await fetch('_probe_opensign.pdf')).arrayBuffer();
  await plugin.LoadFile(new Uint8Array(buf), 'opensign.pdf');
  await plugin.SetPageMode(1, 100);
  await new Promise(r => setTimeout(r, 2000));
  const canvas = document.querySelector('#screen canvas[data-page="0"]');
  if (!canvas) return JSON.stringify({ok:false, reason:'no canvas'});
  const rect = canvas.getBoundingClientRect();
  const pageW = 595.3, pageH = 841.9;
  // 第1个签章 Rect=[60.3,666.9,180.3,786.9] -> 左上原点 y=841.9-786.9=55.0, 120x120 -> 中心(120.3,115)
  const cx = 120.3 * (rect.width/pageW), cy = 115.0 * (rect.height/pageH);
  const hit = plugin.HitTestSeal(0, rect.width, rect.height, cx, cy);
  canvas.dispatchEvent(new MouseEvent('click',{bubbles:true,cancelable:true,button:0,clientX:rect.left+cx,clientY:rect.top+cy}));
  await new Promise(r=>setTimeout(r,150));
  const box = document.getElementById('sealSelectionBox');
  const shown = { display:getComputedStyle(box).display, w:box.style.width, h:box.style.height, left:box.style.left, top:box.style.top };
  // 点空白 (页面右下空白区)
  canvas.dispatchEvent(new MouseEvent('click',{bubbles:true,cancelable:true,button:0,clientX:rect.left+rect.width*0.9,clientY:rect.top+rect.height*0.95}));
  await new Promise(r=>setTimeout(r,100));
  const afterEmpty = getComputedStyle(box).display;
  return JSON.stringify({ ok:true, hitExists: !!hit, shown, afterEmpty });
})()`);
console.log(out);
if (errs.length) console.log('错误:\n'+errs.slice(0,8).join('\n'));
const shot = await page.send('Page.captureScreenshot',{format:'png'});
writeFileSync('out/opensign-click.png', Buffer.from(shot.data,'base64'));
console.log('截图: out/opensign-click.png');
browser.close();
