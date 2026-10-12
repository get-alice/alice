import { defineConfig, fontProviders } from 'astro/config';

export default defineConfig({
  site: 'https://getalice.email',
  trailingSlash: 'never',
  build: { format: 'file' },
  // Keep HTML whitespace rules: prose mixes text and inline links across lines.
  compressHTML: true,
  security: { csp: true },
  markdown: { syntaxHighlight: false },
  fonts: [
    {
      name: 'Newsreader',
      cssVariable: '--font-serif',
      provider: fontProviders.fontsource(),
      weights: ['300 600'],
      styles: ['normal', 'italic'],
      subsets: ['latin'],
      fallbacks: ['Georgia', 'serif'],
    },
    {
      name: 'DM Sans',
      cssVariable: '--font-sans',
      provider: fontProviders.fontsource(),
      weights: ['400 600'],
      styles: ['normal'],
      subsets: ['latin'],
      fallbacks: ['system-ui', 'sans-serif'],
    },
  ],
});
