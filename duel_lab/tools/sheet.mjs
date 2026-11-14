// node sheet.mjs <ms,ms,...> [query] [name]  -> ../shots/<name>.png : a contact sheet of small frames (4 per row), time printed by the HUD
import puppeteer from 'puppeteer-core';
import sharp from 'sharp';
const times = (process.argv[2] || '0').split(',').map(Number), q = process.argv[3] || '', name = process.argv[4] || 'sheet', W = 720, H = 405, COLS = Math.min(4, times.length);
const b = await puppeteer.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: 'new', args: ['--window-size=1440,810', '--use-angle=metal', '--enable-gpu', '--ignore-gpu-blocklist'] });
const p = await b.newPage(); await p.setViewport({ width: 1440, height: 810, deviceScaleFactor: 0.5 });
p.on('console', (m) => { if (m.type() === 'error' && !m.text().includes('404')) console.log('[error]', m.text().slice(0, 300)); else if (m.text().startsWith('script:') || m.text().startsWith('bake')) console.log(m.text()); });
p.on('pageerror', (e) => console.log('[pageerror]', e.message.slice(0, 400)));
await p.goto('http://localhost:8177/index.html?paused=1&' + q, { waitUntil: 'load' });
try { await p.waitForFunction('window.READY === true', { timeout: 60000 }); } catch (e) { console.log('NOT READY:', await p.evaluate(() => document.getElementById('load')?.textContent)); }
const tiles = [];
for (const t of times) { await p.evaluate((t) => window.__setT(t), t); await new Promise((r) => setTimeout(r, 350)); tiles.push(await p.screenshot()); }
console.log('fps', await p.evaluate(() => window.__fps), 'bake', JSON.stringify(await p.evaluate(() => window.__bake)));
await sharp({ create: { width: W * COLS, height: H * Math.ceil(times.length / COLS), channels: 3, background: '#000' } })
  .composite(tiles.map((input, i) => ({ input, left: (i % COLS) * W, top: Math.floor(i / COLS) * H }))).png().toFile(`../shots/${name}.png`);
await b.close();
