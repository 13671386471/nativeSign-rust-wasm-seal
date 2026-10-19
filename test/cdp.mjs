/* =============================================================================
 * cdp.mjs — 极简 Chrome DevTools Protocol 驱动（无第三方依赖）
 * -----------------------------------------------------------------------------
 * 为什么自己写而不是装 agent-browser / puppeteer：
 *   · 本机已有 Chrome，不需要再拉一份 500MB 的 Chromium
 *   · 需求只有三件：抓控制台、执行页面脚本、截图
 *   · Node 22 自带 WebSocket 与 fetch，够用了
 *
 * 提供：launch() → { port, child, close(), newPage(), inspect(url) }
 * ========================================================================== */
'use strict';

import { spawn } from 'node:child_process';
import { existsSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import net from 'node:net';

/**
 * 每次跑测试都用一个全新的临时 profile 目录。
 * ⚠️ 不要试图在脚本里 rmSync 掉它 —— 本环境把 rmSync 接到了回收站工具上，
 *    面对 Chrome profile 这种几万个小文件的目录会直接 ETIMEDOUT。
 *    放在系统临时目录里，交给 OS 清理即可。
 */
export function freshProfile(tag = 'lab') {
  const dir = join(tmpdir(), `wb-${tag}-${Date.now()}-${Math.floor(Math.random() * 1e6)}`);
  mkdirSync(dir, { recursive: true });
  return dir;
}

const CHROME_CANDIDATES = [
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  'C:/Program Files/Microsoft/Edge/Application/msedge.exe',
  '/usr/bin/google-chrome', '/usr/bin/chromium', '/usr/bin/chromium-browser',
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
];

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export function pickChrome() {
  for (const p of CHROME_CANDIDATES) if (existsSync(p)) return p;
  throw new Error('找不到 Chrome/Edge，请在 CHROME_CANDIDATES 里补上路径');
}

export async function findFreePort(start) {
  for (let p = start; p < start + 40; p++) {
    const free = await new Promise((res) => {
      const s = net.createServer();
      s.once('error', () => res(false));
      s.once('listening', () => s.close(() => res(true)));
      s.listen(p, '127.0.0.1');
    });
    if (free) return p;
  }
  throw new Error('找不到空闲端口');
}

export async function waitFor(fn, { timeout = 20000, interval = 200, label = '条件' } = {}) {
  const t0 = Date.now();
  for (;;) {
    try { const v = await fn(); if (v) return v; } catch { /* 继续等 */ }
    if (Date.now() - t0 > timeout) throw new Error(`等待超时：${label}`);
    await sleep(interval);
  }
}

/* ------------------------------ CDP 连接 ------------------------------ */

export class CDP {
  constructor(ws) {
    this.ws = ws;
    this.id = 0;
    this.pending = new Map();
    this.listeners = [];
    ws.addEventListener('message', (ev) => {
      const msg = JSON.parse(ev.data);
      if (msg.id && this.pending.has(msg.id)) {
        const { resolve, reject } = this.pending.get(msg.id);
        this.pending.delete(msg.id);
        msg.error ? reject(new Error(msg.error.message)) : resolve(msg.result);
      } else if (msg.method) {
        for (const l of this.listeners) l(msg);
      }
    });
  }

  static async connect(wsUrl) {
    const ws = new WebSocket(wsUrl);
    await new Promise((res, rej) => {
      ws.addEventListener('open', res, { once: true });
      ws.addEventListener('error', () => rej(new Error('WebSocket 连接失败')), { once: true });
    });
    return new CDP(ws);
  }

  send(method, params = {}, timeoutMs = 30000) {
    const id = ++this.id;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.ws.send(JSON.stringify({ id, method, params }));
      setTimeout(() => {
        if (this.pending.has(id)) { this.pending.delete(id); reject(new Error(`CDP 超时：${method}`)); }
      }, timeoutMs);
    });
  }

  on(fn) { this.listeners.push(fn); }
  close() { try { this.ws.close(); } catch { /* ignore */ } }

  /** 在页面里求值，返回解析后的值 */
  async eval(expression) {
    const r = await this.send('Runtime.evaluate', {
      expression, returnByValue: true, awaitPromise: true,
    });
    if (r.exceptionDetails) {
      throw new Error('页面求值异常：' + (r.exceptionDetails.exception?.description || r.exceptionDetails.text));
    }
    return r.result.value;
  }
}

/* ------------------------------ 启动 / 关闭 ------------------------------ */

export async function launch({ port = 9333, profileDir, width = 1600, height = 1000 } = {}) {
  const chrome = pickChrome();
  const p = await findFreePort(port);
  const child = spawn(chrome, [
    '--headless=new', '--disable-gpu', '--no-sandbox', '--hide-scrollbars',
    '--disable-dev-shm-usage', '--no-first-run', '--no-default-browser-check',
    '--disable-features=Translate,BackForwardCache',
    `--remote-debugging-port=${p}`, `--user-data-dir=${profileDir}`,
    'about:blank',
  ], { stdio: 'ignore' });

  await waitFor(async () => (await fetch(`http://127.0.0.1:${p}/json/version`)).ok, { label: 'Chrome 调试端口就绪' });

  return {
    chrome, port: p, child,
    width, height,
    async newPage() {
      const res = await fetch(`http://127.0.0.1:${p}/json/new?${encodeURIComponent('about:blank')}`, { method: 'PUT' });
      const target = await res.json();
      const cdp = await CDP.connect(target.webSocketDebuggerUrl);
      await cdp.send('Runtime.enable');
      await cdp.send('Page.enable');
      await cdp.send('Network.enable');
      await cdp.send('Emulation.setDeviceMetricsOverride', {
        width, height, deviceScaleFactor: 1, mobile: false,
      });
      cdp.targetId = target.id;
      return cdp;
    },
    async closePage(cdp) {
      cdp.close();
      try { await fetch(`http://127.0.0.1:${p}/json/close/${cdp.targetId}`); } catch { /* ignore */ }
    },
    close() {
      child.kill('SIGKILL');
    },
  };
}

/**
 * 给一个 CDP 页面装上「控制台 / 异常 / 失败请求」采集器。
 * ERR_ABORTED 单独归类 —— pdf.js 读完整篇后主动取消流，浏览器会记成中止，
 * 这是正常行为，不该跟真正的加载失败混在一起。
 */
export function attachCollector(cdp) {
  const log = { errors: [], failedRequests: [], aborted: [] };
  const reqUrls = new Map();
  cdp.on((msg) => {
    if (msg.method === 'Network.requestWillBeSent') {
      reqUrls.set(msg.params.requestId, msg.params.request.url);
    }
    if (msg.method === 'Runtime.consoleAPICalled' && msg.params.type === 'error') {
      log.errors.push('console.error: ' + (msg.params.args || [])
        .map((a) => (a.value !== undefined ? String(a.value) : a.description || a.type)).join(' '));
    }
    if (msg.method === 'Runtime.exceptionThrown') {
      const d = msg.params.exceptionDetails;
      log.errors.push('未捕获异常: ' + (d.exception?.description || d.text));
    }
    if (msg.method === 'Log.entryAdded' && msg.params.entry.level === 'error') {
      log.errors.push(`[${msg.params.entry.source}] ${msg.params.entry.text}`);
    }
    if (msg.method === 'Network.loadingFailed') {
      const u = (reqUrls.get(msg.params.requestId) || '').replace(/^https?:\/\/[^/]+/, '');
      const line = `${msg.params.type} ${msg.params.errorText} ← ${u}`;
      if (msg.params.errorText === 'net::ERR_ABORTED') log.aborted.push(line);
      else log.failedRequests.push(line);
    }
  });
  return log;
}
