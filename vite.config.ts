import { configDefaults, defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
export default defineConfig({
  plugins: [react()],
  server: { port: 1420, strictPort: true, watch: { ignored: ['**/src-tauri/**'] } },
  optimizeDeps: { entries: ['index.html'] },
  test: {
    environment: 'jsdom',
    exclude: [...configDefaults.exclude, '.local/**', 'tools/**'],
  },
});
