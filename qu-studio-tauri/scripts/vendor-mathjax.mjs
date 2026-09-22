// Copy MathJax out of node_modules into `public/vendor/mathjax/`, where
// Vite serves it verbatim.
//
// Why a copy step rather than an import: importing the 1.2 MB minified
// bundle puts it through Vite's dev transform pipeline, which froze the
// page hard enough that `1+1` in the console timed out. `public/` is
// copied byte-for-byte and never transformed.
//
// Why a copy step rather than committing the files: it is 3.1 MB with the
// fonts, and `package-lock.json` already pins the exact version. Vendoring
// it into git would put a second, unpinned copy of the same thing in the
// tree and it would drift from the lockfile the moment anyone updated the
// dependency.
import { cp, mkdir, access } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const src = resolve(here, '../node_modules/mathjax/es5');
const dest = resolve(here, '../public/vendor/mathjax');

try {
  await access(src);
} catch {
  // A clear message rather than a stack trace: the cause is always the
  // same and the fix is one command.
  console.error('vendor-mathjax: mathjax is not installed. Run `npm install` first.');
  process.exit(1);
}

await mkdir(dest, { recursive: true });
// Only what the browser actually fetches: the loader and the font files
// its CHTML output requests. The full `es5` tree is several times larger
// and includes every other output/input combination.
await cp(resolve(src, 'tex-mml-chtml.js'), resolve(dest, 'tex-mml-chtml.js'));
await cp(resolve(src, 'output'), resolve(dest, 'output'), { recursive: true });
console.log(`vendor-mathjax: copied to ${dest}`);
