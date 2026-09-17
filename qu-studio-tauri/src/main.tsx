import React from 'react';
import ReactDOM from 'react-dom/client';
import { ThemeProvider } from '@qu/ui-components';
import App from './App';
import { GuiRunnerWindow } from './GuiRunnerWindow';
import './index.css';

// A designed/hand-written GUI's "Run" opens a second native window
// (`GuiPanel`'s `openRunnerWindow`) pointed at this same `index.html`
// with `?guiRunner=1&session=<id>` in the URL, rather than a second HTML
// entry point Vite would need its own build config for. Checked before
// the rest of the app boots, so the runner window never mounts the full
// Studio chrome (sidebar, tabs, code editor) it has no use for.
const isGuiRunner = new URLSearchParams(window.location.search).get('guiRunner') === '1';

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <ThemeProvider defaultTheme="dark">
      {isGuiRunner ? <GuiRunnerWindow /> : <App />}
    </ThemeProvider>
  </React.StrictMode>
);
