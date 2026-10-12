import js from '@eslint/js';
import eslintReact from '@eslint-react/eslint-plugin';
import reactHooks from 'eslint-plugin-react-hooks';
import globals from 'globals';
import tseslint from 'typescript-eslint';

// Hook rules come from React's own plugin (backed by the React Compiler); @eslint-react carries ports of
// the same rules, which would report every finding twice.
const duplicateHookRules = Object.fromEntries(['error-boundaries', 'exhaustive-deps', 'purity', 'rules-of-hooks', 'set-state-in-effect', 'set-state-in-render', 'static-components', 'unsupported-syntax', 'use-memo'].map(rule => [`@eslint-react/${rule}`, 'off']));

export default tseslint.config(
  { ignores: ['dist/', 'target/', 'vendor/', 'node_modules/', 'apps/desktop/src-tauri/', 'apps/website/', '.claude/', '.agents/'] },
  {
    name: 'alice/desktop',
    files: ['apps/desktop/src/**/*.{ts,tsx}', 'vite.config.ts'],
    extends: [js.configs.recommended, tseslint.configs.strictTypeChecked, tseslint.configs.stylisticTypeChecked, eslintReact.configs['recommended-type-checked'], reactHooks.configs.flat.recommended],
    languageOptions: { parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname }, globals: globals.browser },
    linterOptions: { reportUnusedDisableDirectives: 'error' },
    rules: {
      ...duplicateHookRules,
      // Refs here are named for what they hold (`dialog`, `timer`), not suffixed `Ref`.
      '@eslint-react/naming-convention-ref-name': 'off',
      // Promise-returning handlers are the norm for Tauri `invoke` and daemon calls; `void` marks the fire-and-forget ones.
      '@typescript-eslint/no-floating-promises': ['error', { ignoreVoid: true }],
      '@typescript-eslint/no-misused-promises': ['error', { checksVoidReturn: { attributes: false } }],
      '@typescript-eslint/no-confusing-void-expression': ['error', { ignoreArrowShorthand: true, ignoreVoidOperator: true }],
      '@typescript-eslint/restrict-template-expressions': ['error', { allowNumber: true }],
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_', caughtErrors: 'none' }],
      // `.catch(() => {})` deliberately drops failures the next poll or save retries.
      '@typescript-eslint/no-empty-function': ['error', { allow: ['arrowFunctions'] }],
      // `||` on strings, numbers and booleans is deliberate: an empty field or zero falls back like a missing one.
      '@typescript-eslint/prefer-nullish-coalescing': ['error', { ignorePrimitives: { string: true, number: true, boolean: true } }],
      // Reports `list[i]?.x` as needless, since tsconfig leaves `noUncheckedIndexedAccess` off; the checks guard real gaps.
      '@typescript-eslint/no-unnecessary-condition': 'off',
    },
  },
  {
    name: 'alice/scripts',
    files: ['scripts/**/*.mjs', 'eslint.config.js'],
    extends: [js.configs.recommended],
    languageOptions: { globals: globals.node },
  },
);
