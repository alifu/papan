// @ts-check

import eslint from '@eslint/js';
import tseslint from 'typescript-eslint';

export default tseslint.config(
  {
    ignores: [
      'node_modules',
      'dist',
      'build',
      'src-tauri/target',
      // Embedded in the Rust binary and run in QuickJS, not in the webview.
      'src-tauri/src/script/*.js',
      'coverage',
      'playwright-report',
      'test-results',
      'src/ipc/bindings.ts',
      'graphify-out',
    ],
  },
  eslint.configs.recommended,
  tseslint.configs.recommended,
);
