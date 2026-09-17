# Qu UI Components

**Modern, beautiful UI components for QuStudio & MattyStudio IDEs**

A shared component library built with React, TypeScript, and Framer Motion for building stunning IDE interfaces with glass morphism design.

## Workspace inspection

`VariableExplorer` accepts display `variables` and optional `numericData` from
Qu's execution response. It supports name/type search, type filtering, sorting,
numeric summaries, sampled profiles, paged matrix tables, plot previews, and
full-precision CSV downloads. Numeric matrices use column-major storage with
`shape: [rows, columns]`; missing/non-finite values are excluded from statistics.
Vector plot previews preserve sampled extrema and gaps; matrix previews show
the first 50 rows and columns. The data table and CSV cover the full dataset.

`FigureViewer` displays Qu's original SVG/PNG figures with thumbnails, fit/zoom,
original-file downloads, and an accessible enlarged dialog. Inline SVG retains
the renderer's point tooltips. Escape closes the dialog; the focused canvas
accepts left/right arrows to navigate figures.

`PlotViewer` provides Grey, Minimal, and Classic styles inspired by
[ggplot2's complete themes](https://ggplot2.tidyverse.org/reference/ggtheme.html),
with light/dark variants, Viridis for continuous fields, pan/zoom, reset,
fullscreen, and PNG/SVG export. Axis limits are expressed in data units,
including logarithmic axes. WebGL layers remain rasterized inside SVG exports.
Pass a numeric height or `height="100%"` inside a container with a defined height.

Validation: run `npm test` here and `npm run build` in `../qu-studio-tauri`.

## 🎨 Features

- ✨ **Glass Morphism Design** - Beautiful translucent panels with blur effects
- 🌓 **Dark/Light Themes** - Auto-switch with system preference
- 🎭 **Smooth Animations** - 60fps transitions with Framer Motion
- ⚡ **Type-Safe** - Full TypeScript support
- 📦 **Tree-Shakeable** - Only import what you need
- ♿ **Accessible** - Keyboard navigation & screen reader support

## 📦 Installation

```bash
npm install @qu/ui-components
```

## 🚀 Quick Start

```tsx
import { IDEShell, CodeEditor, PlotViewer, FileTree } from '@qu/ui-components';
import { ThemeProvider } from '@qu/ui-components';

function App() {
  return (
    <ThemeProvider defaultTheme="dark">
      <IDESShell>
        <CodeEditor language="qu" value="x = 1 to 10" />
        <PlotViewer data={[{ x: [1,2,3], y: [4,5,6], type: 'line' }]} />
        <FileTree files={files} />
      </IDESShell>
    </ThemeProvider>
  );
}
```

## 🧩 Components

### Core Components

| Component | Description |
|---|---|
| `CodeEditor` | Monaco-based code editor with Qu syntax highlighting |
| `PlotViewer` | Interactive Plotly.js plot viewer (2D/3D) |
| `FileTree` | File explorer with drag-drop support |
| `Terminal` | Integrated REPL/console |
| `TabBar` | Multi-file tab bar |
| `StatusBar` | Editor status information |
| `SplitView` | Resizable split panes |
| `CommandPalette` | Ctrl+P quick actions |
| `VariableExplorer` | Data inspector |
| `IDESShell` | Complete IDE layout |

### Hooks

| Hook | Description |
|---|---|
| `useTheme` | Theme management (light/dark/system) |
| `useIDEStore` | Zustand state management for IDE |

### Utilities

| Utility | Description |
|---|---|
| `cn()` | Class name composition |
| `glassStyle` | Glass morphism CSS |
| `fadeVariants` | Framer Motion animations |

## 🎨 Theming

```tsx
<ThemeProvider defaultTheme="system">
  <App />
</ThemeProvider>
```

Themes:
- `'light'` - Light theme
- `'dark'` - Dark theme  
- `'system'` - Auto-switch with OS (default)

## 📝 Examples

### Code Editor

```tsx
<CodeEditor
  language="qu"
  value={code}
  onChange={setCode}
  onRun={handleRun}
  onSave={handleSave}
  showLineNumbers={true}
  showMinimap={true}
  theme="dark"
/>
```

### Plot Viewer

```tsx
<PlotViewer
  data={[
    { x: [1, 2, 3, 4], y: [1, 4, 9, 16], type: 'scatter', mode: 'lines+markers' }
  ]}
  title="My Plot"
  xlabel="X Axis"
  ylabel="Y Axis"
  theme="dark"
  onExport={(format) => console.log('Exported as', format)}
/>
```

### File Tree

```tsx
<FileTree
  files={[
    {
      id: '1',
      name: 'src',
      type: 'folder',
      path: '/src',
      children: [
        { id: '2', name: 'main.qu', type: 'file', path: '/src/main.qu' }
      ]
    }
  ]}
  selectedFile="2"
  onSelect={(file) => console.log('Selected:', file)}
/>
```

## 🛠 Development

```bash
cd qu-ui-components
npm install
npm run dev
```

### If you are here because Qu Studio showed a blank page

Qu Studio consumes this package **as source**, via a Vite alias pointing at
`qu-ui-components/src` -- not as a built artifact. Vite therefore resolves
these files' imports starting from *this* directory and walking upward, so
it never reaches `qu-studio-tauri/node_modules`, where the Studio's own
`npm install` hoists everything.

If `qu-ui-components/node_modules` is empty, the Studio dev server serves a
blank page behind a Vite overlay -- `Failed to resolve import "zustand"`,
then `plotly.js/dist/plotly`, and so on, one dependency at a time. Nothing
in that message points here.

`npm install` in `qu-studio-tauri` now installs this package too, via a
`postinstall` hook, so a fresh clone works from either directory. For an
existing checkout, `npm install` here by hand is the fix.

### Build

```bash
npm run build
```

## 📐 Design Principles

1. **Native Feel** - Each IDE feels at home on its platform
2. **Shared DNA** - Same visual language across QuStudio & MattyStudio
3. **Performance** - 60fps animations, instant response
4. **Accessibility** - Keyboard nav, screen reader support
5. **Extensible** - Plugin system ready

## 🎯 Tech Stack

- **React 18** - UI framework
- **TypeScript** - Type safety
- **Framer Motion** - Animations
- **Monaco Editor** - Code editing (VS Code's editor)
- **Plotly.js** - Interactive plots
- **Zustand** - State management
- **Tailwind CSS** - Styling
- **Vite** - Build tool

## 📄 License

MIT

## 🙏 Credits

Built for the Qu Language project - a scientific scripting language for signal processing, data science, and machine learning.
