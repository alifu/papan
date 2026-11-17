// Renders design/app-icon.svg to a 1024x1024 PNG with Playwright Chromium,
// then regenerates the desktop app icons in src-tauri/icons via `tauri icon`.
// Usage: node scripts/render-icon.mjs

import { execFileSync } from 'node:child_process';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import process from 'node:process';
import { chromium } from '@playwright/test';

const ICON_SIZE = 1024;
const ROOT = resolve(import.meta.dirname, '..');
const SVG_PATH = join(ROOT, 'design', 'app-icon.svg');
const ICONS_DIR = join(ROOT, 'src-tauri', 'icons');
const MOBILE_DIRS = ['android', 'ios'];

function pageHtml(svg) {
  return `<!doctype html><html><head><style>
    html, body { margin: 0; padding: 0; background: transparent; }
    svg { display: block; width: ${ICON_SIZE}px; height: ${ICON_SIZE}px; }
  </style></head><body>${svg}</body></html>`;
}

async function renderPng(svg, outPath) {
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage({
      viewport: { width: ICON_SIZE, height: ICON_SIZE },
      deviceScaleFactor: 1,
    });
    await page.setContent(pageHtml(svg));
    await page.screenshot({ path: outPath, omitBackground: true });
  } finally {
    await browser.close();
  }
}

async function main() {
  const svg = await readFile(SVG_PATH, 'utf8');
  const workDir = await mkdtemp(join(tmpdir(), 'papan-icon-'));
  const pngPath = join(workDir, 'app-icon.png');
  try {
    await renderPng(svg, pngPath);
    execFileSync('npx', ['tauri', 'icon', pngPath, '-o', ICONS_DIR], {
      cwd: ROOT,
      stdio: 'inherit',
    });
    await Promise.all(
      MOBILE_DIRS.map((dir) => rm(join(ICONS_DIR, dir), { recursive: true, force: true })),
    );
  } finally {
    await rm(workDir, { recursive: true, force: true });
  }
}

main().catch((error) => {
  process.stderr.write(`render-icon failed: ${error instanceof Error ? error.stack : String(error)}\n`);
  process.exitCode = 1;
});
