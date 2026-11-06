import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Configuración recomendada para Tauri 2.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**', '**/crates/**'] },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  build: {
    target: 'chrome105', // WebView2 en Windows
    minify: 'esbuild',
    sourcemap: false,
  },
});
