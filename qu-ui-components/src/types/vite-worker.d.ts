// Vite's `?worker` import suffix (used in CodeEditor.tsx to bundle Monaco's
// editor worker) is a build-time transform, not a real module path, so
// `tsc` cannot resolve it on its own. Vite ships these types in
// `vite/client`, but adding that to `types` here would pull in the whole
// client ambient surface for one import -- so this declares just the shape
// actually used, in the same spirit as `react-plotly.d.ts` next door.
//
// The transform turns the import into a constructor for a dedicated Worker,
// which is why the default export is a class rather than a URL string.
declare module '*?worker' {
  const WorkerConstructor: {
    new (options?: { name?: string }): Worker;
  };
  export default WorkerConstructor;
}
