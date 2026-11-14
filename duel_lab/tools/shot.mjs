// node shot.mjs <ms,ms,...> [query]  -> ../shots/t<ms>.png ; prints console errors
import puppeteer from 'puppeteer-core';
const times = (process.argv[2] || '0').split(',').map(Number), q = process.argv[3] || '';
const b = await puppeteer.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: 'new',
  args: ['--window-size=1440,810', '--use-angle=metal', '--enable-gpu', '--ignore-gpu-blocklist'] });
const p = await b.newPage(); await p.setViewport({ width: 1440, height: 810 });
p.on('console', (m) => { if (m.type() === 'error' || m.type() === 'warning') console.log('[' + m.type() + ']', m.text().slice(0, 300)); });
p.on('pageerror', (e) => console.log('[pageerror]', e.message.slice(0, 400)));
p.on('requestfailed', (r) => console.log('[failed]', r.url().slice(-60)));
await p.goto('http://localhost:8177/index.html?paused=1&' + q, { waitUntil: 'load' });
try { await p.waitForFunction('window.READY === true', { timeout: 60000 }); } catch (e) { console.log('NOT READY:', await p.evaluate(() => document.getElementById('load')?.textContent)); }
for (const t of times) { await p.evaluate((t) => { window.__setT && window.__setT(t); }, t); await new Promise((r) => setTimeout(r, 700)); await p.screenshot({ path: `../shots/t${t}.png` }); console.log('shot', t); }
await b.close();
