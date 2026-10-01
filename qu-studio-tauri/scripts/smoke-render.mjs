// Smoke test for the built Studio frontend: serve dist/, load it in
// headless Chromium, and fail unless the app actually mounts and every
// top-level tab opens without crashing it.
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
const page = await browser.newPage({ viewport: { width: 1400, height: 900 } })
const failures = []
const expected = (s) => /__TAURI_IPC__|__TAURI_METADATA__|Tauri invoke error/.test(s)
page.on('pageerror', (e) => { if (!expected(String(e))) failures.push(String(e.stack || e)) })
page.on('console', (m) => { if (m.type() === 'error' && !expected(m.text())) failures.push(m.text()) })
await page.goto(`http://localhost:${port}/`, { waitUntil: 'load' })
await page.waitForTimeout(3000)
const rendered = () => page.evaluate(() => (document.getElementById('root')?.innerHTML.length ?? 0) > 1000)
const mounted = await rendered()
// Every top-level mode, not just the first screen: 0.4.5's Interactive tab
// crashed React (a CommonJS default import, see PlotViewer.tsx) and left a
// black window while the start page was fine.
const TABS = ['Code', 'DSP', 'Designer', 'Interactive', 'ML', 'SeriPlot', 'Files']
if (mounted) {
  for (const tab of TABS) {
    const before = failures.length
    await page.getByText(tab, { exact: true }).first().click({ timeout: 5000 }).catch((e) => failures.push(`could not click the ${tab} tab: ${e.message.split('\n')[0]}`))
    await page.waitForTimeout(1200)
    if (!(await rendered())) { failures.push(`the ${tab} tab left #root empty`); break }
    if (failures.length > before) failures[before] = `[${tab} tab] ` + failures[before]
  }
}
await browser.close()
server.close()

if (!mounted) failures.unshift('the app did not mount: #root is empty')
if (failures.length) {
  console.error('Studio smoke render FAILED:\n' + failures.join('\n---\n'))
  process.exit(1)
}
console.log(`Studio smoke render: mounted, ${TABS.length} tabs opened, no unexpected errors`)
