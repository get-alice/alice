import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

function tokenFile() {
  const root = process.env.ALICE_DATA_DIR || (process.platform === 'darwin'
    ? join(homedir(), 'Library', 'Application Support', 'Alice')
    : process.platform === 'win32' ? join(process.env.LOCALAPPDATA || homedir(), 'Alice')
    : join(process.env.XDG_DATA_HOME || join(homedir(), '.local', 'share'), 'Alice'));
  return join(root, 'daemon.token');
}

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    strictPort: true,
    proxy: {
      '/api': {
        target: `http://127.0.0.1:${process.env.ALICE_PORT || 4815}`,
        rewrite: path => path.replace(/^\/api/, ''),
        configure(proxy) {
          proxy.on('proxyReq', request => {
            try { request.setHeader('Authorization', `Bearer ${readFileSync(tokenFile(), 'utf8').trim()}`); }
            catch { /* The daemon may still be starting. The UI retries. */ }
          });
        },
      },
    },
  },
});
