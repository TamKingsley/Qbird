import { defineConfig } from 'vite';
import { resolve } from 'node:path';

export default defineConfig({
  clearScreen: false,
  server: { host: '127.0.0.1', port: 1420, strictPort: true },
  build: {
    target: 'es2022',
    rollupOptions: {
      // The capture overlay runs in its own frameless fullscreen window.
      input: {
        main: resolve(__dirname, 'index.html'),
        records: resolve(__dirname, 'records.html'),
        capture: resolve(__dirname, 'capture.html'),
        notify: resolve(__dirname, 'notify.html'),
      },
    },
  },
});
