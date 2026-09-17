import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'path'

export default defineConfig({
  plugins: [react()],
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
    minify: 'esbuild',
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
