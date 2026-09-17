// Seven call sites across this package import `glassStyle` from this
// dedicated module path, but the implementation actually lives in `cn.ts`
// alongside `cn`/`gradientAccent`/the animation variants — re-export it
// here rather than rewriting every import site.
export { glassStyle } from './cn';
