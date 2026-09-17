import config from './vite.config';
export default { ...config, cacheDir: `${process.env.LOCALAPPDATA}/QuStudio/figure-review-vite`, server: { port: 1421, strictPort: true, host: '127.0.0.1' } };
