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
import { fileURLToPath } from 'node:url'
import { chromium } from 'playwright'

const root = fileURLToPath(new URL('../dist/', import.meta.url))
const types = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.wasm': 'application/wasm', '.json': 'application/json', '.ttf': 'font/ttf', '.woff2': 'font/woff2' }
const server = createServer(async (req, res) => {
  const rel = normalize(decodeURIComponent(new URL(req.url, 'http://x').pathname)).replace(/^[\\/]+/, '')
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
// File viewers: drop an SVG (with a script and an onload handler that must
// NOT survive), a PDF, a .qu file and an unsupported file onto the window
// the way a browser delivers them (HTML5 drop; Tauri's native file-drop
// feeds the same openers). Back on the Code tab first.
let viewerChecks = 0
if (mounted) {
  const check = async (label, fn) => {
    try { await fn(); viewerChecks++ } catch (e) { failures.push(`[viewers] ${label}: ${String(e.message || e).split('\n')[0]}`) }
  }
  await page.getByText('Code', { exact: true }).first().click({ timeout: 5000 }).catch(() => {})
  await page.waitForTimeout(500)
  const drop = (files, kind = 'drop') => page.evaluate(({ files, kind }) => {
    const dt = new DataTransfer()
    for (const f of files) dt.items.add(new File([f.body], f.name, { type: f.type }))
    window.dispatchEvent(new DragEvent(kind, { dataTransfer: dt, bubbles: true, cancelable: true }))
  }, { files, kind })

  await check('drop overlay appears on dragenter', async () => {
    await drop([{ name: 'x.svg', body: '<svg/>', type: 'image/svg+xml' }], 'dragenter')
    await page.waitForSelector('[data-testid=drop-overlay]', { timeout: 3000 })
  })
  const evil = '<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" onload="window.__pwned=1"><script>window.__pwned=2</script><foreignObject><div>x</div></foreignObject><rect id="ok" width="200" height="100" fill="red" onclick="window.__pwned=3"/></svg>'
  await check('svg drop renders sanitised in the viewer', async () => {
    await drop([{ name: 'evil.svg', body: evil, type: 'image/svg+xml' }])
    await page.waitForSelector('[data-testid=svg-viewer] svg rect#ok', { timeout: 5000 })
    const html = await page.evaluate(() => document.querySelector('[data-testid=svg-viewer]').innerHTML)
    if (/<script|onload|onclick|foreignObject/i.test(html)) throw new Error('unsanitised markup survived: ' + html.slice(0, 200))
    if (await page.evaluate(() => window.__pwned)) throw new Error('script inside the SVG executed')
    if (await page.locator('[data-testid=drop-overlay]').count()) throw new Error('overlay stuck after drop')
  })
  await check('svg viewer zooms with the wheel and fits', async () => {
    const stage = page.locator('.qu-viewer-stage').first()
    const before = await page.locator('.qu-viewer-zoom').first().innerText()
    const box = await stage.boundingBox()
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2)
    await page.mouse.wheel(0, -400)
    await page.waitForTimeout(200)
    const after = await page.locator('.qu-viewer-zoom').first().innerText()
    if (before === after) throw new Error(`wheel did not change zoom (${before})`)
    await page.getByText('100%', { exact: true }).first().click()
    await page.waitForTimeout(400)
    if ((await page.locator('.qu-viewer-zoom').first().innerText()) !== '100%') throw new Error('100% button did not give 100%')
  })
  await check('view source opens the svg as a text tab', async () => {
    await page.getByText('View source').first().click()
    await page.waitForSelector('.monaco-editor', { timeout: 15000 })
    if (!(await page.getByText('evil.svg (source)').count())) throw new Error('no "(source)" tab')
  })
  await check('pdf drop shows the viewer (embed or fallback)', async () => {
    await drop([{ name: 'doc.pdf', body: '%PDF-1.4\n1 0 obj<<>>endobj\ntrailer<<>>\n%%EOF', type: 'application/pdf' }])
    await page.waitForSelector('[data-testid=pdf-viewer]', { timeout: 5000 })
    const ok = await page.evaluate(() => !!document.querySelector('[data-testid=pdf-viewer] embed, [data-testid=pdf-viewer] .qu-viewer-fallback'))
    if (!ok) throw new Error('neither embed nor fallback rendered')
  })
  await check('unsupported file is refused with a toast naming it', async () => {
    await drop([{ name: 'virus.exe', body: 'MZ', type: 'application/octet-stream' }])
    await page.waitForSelector('.qu-toast:has-text("virus.exe")', { timeout: 3000 })
  })
  await check('a .qu drop opens an editor tab', async () => {
    await drop([{ name: 'dropped_script.qu', body: 'x = 1\nprint(x)\n', type: 'text/plain' }])
    await page.waitForSelector('[role=tab]:has-text("dropped_script.qu")', { timeout: 5000 })
  })
}

await browser.close()
server.close()

if (!mounted) failures.unshift('the app did not mount: #root is empty')
if (failures.length) {
  console.error('Studio smoke render FAILED:\n' + failures.join('\n---\n'))
  process.exit(1)
}
console.log(`Studio smoke render: mounted, ${TABS.length} tabs opened, ${viewerChecks} viewer/drop checks passed, no unexpected errors`)
