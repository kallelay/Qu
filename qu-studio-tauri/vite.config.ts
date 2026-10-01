import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'path'
import { readFileSync } from 'fs'

// The version shown in the status bar, from package.json at build time
// (it was a hard-coded "v0.1.0" through 0.4.4).
const version = JSON.parse(readFileSync(path.resolve(__dirname, 'package.json'), 'utf8')).version

export default defineConfig({
  plugins: [react()],
  define: { __APP_VERSION__: JSON.stringify(version) },
  // tauri.conf.json's build.devPath is hardcoded to http://localhost:1420
  // (the standard create-tauri-app convention) -- without pinning the vite
  // dev server to that exact port, `tauri dev` waits for a server that's
  // actually listening somewhere else (vite silently drifts to the next
  // free port, e.g. 5173/5174/...) and times out after 180s.
  server: {
    port: 1420,
    strictPort: true,
  },
  resolve: {
    // ONE React. `@qu/ui-components` is aliased to its source, and that
    // source's own `import 'react'` resolves from qu-ui-components/node_modules
    // -- a second copy. Two Reacts in one bundle is the classic
    // "Cannot read properties of null (reading 'useState')" crash at mount:
    // Qu Studio 0.4.4 (the first build on Vite 8) opened to a blank dark
    // window because of exactly this. `dedupe` makes every import of these
    // resolve from this project's root.
    dedupe: ['react', 'react-dom', 'react/jsx-runtime'],
    alias: {
      '@': path.resolve(__dirname, './src'),
      // `__dirname`, not a bare relative path: `path.resolve('../...')`
      // resolves against process.cwd(), so it only happened to be correct
      // while vite was launched from this directory, and silently pointed
      // somewhere else otherwise (`npm --prefix`, a workspace root, any
      // `--config` invocation). The `@` alias above always had this right.
      '@qu/ui-components': path.resolve(__dirname, '../qu-ui-components/src'),
    },
  },
  build: {
    target: 'esnext',
    // Vite 8's own minifier (Oxc); `'esbuild'` would need esbuild installed
    // separately now, and Vite 8 no longer bundles it.
    minify: 'oxc',
  },
  // monaco-editor's own bundled worker code trips esbuild's dependency
  // pre-bundler ("Invalid regular expression: /\\\/: \ at end of pattern"),
  // a known Vite+Monaco interaction, so it stays out of pre-bundling.
  //
  // The second half of this note used to say monaco-editor "isn't even used
  // at runtime with this loader", because @monaco-editor/react's default
  // loader fetched Monaco from jsdelivr instead. That is no longer true and
  // was never a good state for a desktop app: Monaco is now imported and
  // bundled, and `loader.config({ monaco })` in CodeEditor.tsx points the
  // loader at it. Excluding it here only skips DEV pre-bundling (Vite serves
  // it as source instead); production builds are unaffected either way.
  optimizeDeps: {
    exclude: ['monaco-editor', '@monaco-editor/react'],
  },
})
