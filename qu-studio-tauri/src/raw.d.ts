declare module '*?raw' {
  const content: string;
  export default content;
}

// Injected by vite.config.ts `define` from package.json.
declare const __APP_VERSION__: string;
