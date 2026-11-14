// node rewind_check.mjs : time travel must be exact. Pose a set of times, scrub to the end and back in a scrambled
// order, and compare a digest of every bone, every visible body and the camera with the first visit.
import puppeteer from 'puppeteer-core';
const times = [1000, 4200, 6150, 7400, 9150, 13200, 15400, 19100, 21500, 24700, 25600, 27500, 31000];
const b = await puppeteer.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: 'new', args: ['--window-size=1440,810', '--use-angle=metal', '--enable-gpu', '--ignore-gpu-blocklist'] });
const p = await b.newPage(); await p.setViewport({ width: 1440, height: 810 });
await p.goto('http://localhost:8177/index.html?paused=1', { waitUntil: 'load' }); await p.waitForFunction('window.READY === true', { timeout: 60000 });
const visit = async (t) => { const d = await p.evaluate((t) => { window.__setT(t); return window.__digest(t); }, t); await new Promise((r) => setTimeout(r, 120)); return [d]; };
const first = {}; for (const t of times) first[t] = await visit(t);
let bad = 0; for (const t of [...times].reverse().concat(times.filter((_, i) => i % 2), times.filter((_, i) => i % 2 === 0))) { const [d] = await visit(t); if (d !== first[t][0]) { bad++; console.log('STATE MISMATCH at', t, first[t][0], d); } }
console.log(`${times.length} times, each revisited twice out of order: ${bad ? bad + ' STATE MISMATCHES' : 'state identical every time'}`);
await b.close(); process.exit(bad ? 1 : 0);
