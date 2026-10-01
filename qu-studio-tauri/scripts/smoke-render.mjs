// Smoke test for the built Studio frontend: serve dist/, load it in
// headless Chromium, and fail unless the app actually mounts.
//
// Qu Studio 0.4.4 shipped opening to a blank dark window: two copies of
// React in the bundle (see vite.config.ts `dedupe`) crashed React at mount,
// and nothing between `vite build` and the installer ever loaded the page.
// Outside Tauri there is no IPC, so errors about __TAURI_IPC__/__TAURI_
// METADATA__ are expected and ignored; anything else thrown at load fails.
//
//   npx vite build && node scripts/smoke-render.mjs
import { createServer } from 'node:http'
import { readFile } from 'node:fs/promises'
import { extname, join, normalize } from 'node:path'
import { chromium } from 'playwright'

const root = new URL('../dist/', import.meta.url).pathname
const types = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.wasm': 'application/wasm', '.json': 'application/json', '.ttf': 'font/ttf', '.woff2': 'font/woff2' }
const server = createServer(async (req, res) => {
  const rel = normalize(decodeURIComponent(new URL(req.url, 'http://x').pathname)).replace(/^\/+/, '')
  try {
    const file = rel && rel !== '.' ? rel : 'index.html'
    const body = await readFile(join(root, file))
    res.writeHead(200, { 'content-type': types[extname(file)] || 'application/octet-stream' })
    res.end(body)
  } catch {
    res.writeHead(404)
    res.end()
  }
}).listen(0)
const port = server.address().port

const browser = await chromium.launch(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {})
const page = await browser.newPage()
const failures = []
const expected = (s) => /__TAURI_IPC__|__TAURI_METADATA__|Tauri invoke error/.test(s)
page.on('pageerror', (e) => { if (!expected(String(e))) failures.push(String(e.stack || e)) })
page.on('console', (m) => { if (m.type() === 'error' && !expected(m.text())) failures.push(m.text()) })
await page.goto(`http://localhost:${port}/`, { waitUntil: 'load' })
await page.waitForTimeout(3000)
const mounted = await page.evaluate(() => (document.getElementById('root')?.innerHTML.length ?? 0) > 1000)
await browser.close()
server.close()

if (!mounted) failures.unshift('the app did not mount: #root is empty')
if (failures.length) {
  console.error('Studio smoke render FAILED:\n' + failures.join('\n---\n'))
  process.exit(1)
}
console.log('Studio smoke render: mounted, no unexpected errors')
