# 🚀 Getting Started with QuStudio

## Quick Start (5 minutes)

### Prerequisites

1. **Node.js 18+** - [Download](https://nodejs.org/)
2. **Rust** - [Install via rustup](https://rustup.rs/)
3. **Git** - Already installed

### Step 1: Install Dependencies

```bash
# Navigate to QuStudio
cd qu-studio-tauri            # from the repository root

# Install Node.js dependencies
npm install

# Install Tauri CLI (one-time)
cargo install tauri-cli
```

### Step 2: Run Development Mode

```bash
# This will:
# - Start Vite dev server
# - Launch Tauri desktop app
# - Enable hot-reload
npm run tauri dev
```

**First build time:** ~2-3 minutes  
**Subsequent builds:** ~10-20 seconds

### Step 3: Try the REPL (Instant!)

```bash
# Already compiled and working:
cd .                          # the repository root
QuRepl.bat

# Or manually:
engine\target\debug\qu.exe repl
```

## Project Structure

```
qu-studio-tauri/
├── src/                    # React frontend
│   ├── main.tsx           # Entry point
│   ├── App.tsx            # Main app component
│   └── index.css          # Global styles
├── src-tauri/             # Rust backend
│   ├── src/
│   │   └── main.rs        # Tauri commands
│   ├── Cargo.toml         # Rust dependencies
│   └── tauri.conf.json    # Tauri config
├── index.html             # HTML template
├── package.json           # Node dependencies
├── vite.config.ts         # Vite config
└── tailwind.config.js     # Tailwind config
```

## Available Commands

### Development

```bash
# Run dev server (web only)
npm run dev

# Run Tauri app (desktop)
npm run tauri dev

# Preview production build
npm run preview
```

### Production

```bash
# Build web assets
npm run build

# Build Tauri app (creates installer)
npm run tauri build
```

**Output:** `src-tauri/target/release/Qu Studio.exe`

## Using the Shared Component Library

The `@qu/ui-components` package is automatically linked:

```tsx
import { CodeEditor, PlotViewer, FileTree } from '@qu/ui-components';

function MyComponent() {
  return (
    <div>
      <CodeEditor language="qu" value="x = 1 to 10" />
      <PlotViewer data={[{ x: [1,2,3], y: [4,5,6] }]} />
    </div>
  );
}
```

### Component Library Development

```bash
cd ../qu-ui-components
npm install
npm run dev  # Storybook dev server
npm run build  # Build library
```

## Configuration

### Theme

Edit `tailwind.config.js` to customize colors:

```js
theme: {
  extend: {
    colors: {
      qu: {
        dark: {
          accent: '#3987e5',  // Change accent color
        }
      }
    }
  }
}
```

### Window Size

Edit `src-tauri/tauri.conf.json`:

```json
"windows": [{
  "width": 1400,
  "height": 900,
  "minWidth": 800,
  "minHeight": 600
}]
```

### Tauri Permissions

Edit `src-tauri/tauri.conf.json` -> `allowlist`:

```json
"allowlist": {
  "fs": { "all": true },      // Full filesystem access
  "shell": { "open": true },  // Open external URLs
  "process": { "all": true }  // Spawn processes
}
```

## Debugging

### Frontend (React)

1. Open DevTools in the Tauri app (Ctrl+Shift+I)
2. Use React DevTools extension
3. Check Console for errors

### Backend (Rust)

```bash
# Run with verbose logging
RUST_LOG=debug npm run tauri dev

# Check Rust compiler errors
cargo check --manifest-path src-tauri/Cargo.toml
```

## Common Issues

### Issue: "Tauri CLI not found"

```bash
cargo install tauri-cli
```

### Issue: "Module not found: @qu/ui-components"

```bash
cd ../qu-ui-components
npm install
npm run build
cd ../qu-studio-tauri
npm install
```

### Issue: Build fails on Windows

```bash
# Install Windows Build Tools
npm install --global windows-build-tools

# Or via Chocolatey
choco install visualstudio2022-buildtools
```

### Issue: Rust version mismatch

```bash
rustup update
rustup default stable
```

## Testing

### Unit Tests (Rust)

```bash
cd src-tauri
cargo test
```

### Component Tests (React)

```bash
npm install --save-dev @testing-library/react
npm test
```

## Deployment

### Build for Windows

```bash
npm run tauri build
```

**Output locations:**
- Installer: `src-tauri/target/release/bundle/msi/Qu Studio_0.1.0_x64_en-US.msi`
- EXE: `src-tauri/target/release/Qu Studio.exe`

### Build for macOS

```bash
npm run tauri build -- --target universal-apple-darwin
```

### Build for Linux

```bash
npm run tauri build
```

## Performance Tips

1. **Use production build** for testing: `npm run build`
2. **Disable dev tools** in production
3. **Minimize Rust <-> JS calls** (they're async)
4. **Lazy load components** with `React.lazy()`
5. **Use React.memo()** for expensive components

## Next Steps

1. ✅ Try the working REPL: `QuRepl.bat`
2. ✅ Build QuStudio: `npm run tauri dev`
3. ✅ Explore components in `qu-ui-components/`
4. ✅ Read `VISUAL_DESIGN.md` for design specs
5. ✅ Check `IMPLEMENTATION_COMPLETE.md` for status

## Resources

- [Tauri Docs](https://tauri.app/v1/)
- [React Docs](https://react.dev/)
- [Monaco Editor](https://microsoft.github.io/monaco-editor/)
- [Plotly.js](https://plotly.com/javascript/)
- [Framer Motion](https://www.framer.com/motion/)
- [Zustand](https://github.com/pmndrs/zustand)

## Support

- Issues: GitHub Issues
- Chat: Discord/Slack (if applicable)
- Docs: `docs/` folder

---

**Happy coding! 🎉**
