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
