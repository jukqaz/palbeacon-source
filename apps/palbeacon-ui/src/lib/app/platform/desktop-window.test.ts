import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

interface TauriWindowConfig {
  app: { windows: Array<{ minWidth?: number; minHeight?: number; resizable?: boolean }> };
}

describe('Windows desktop window contract', () => {
  it('stays resizable without entering unsupported compact layouts', async () => {
    const config = JSON.parse(
      await readFile(
        resolve(process.cwd(), '../palbeacon-desktop/src-tauri/tauri.conf.json'),
        'utf8',
      ),
    ) as TauriWindowConfig;
    const mainWindow = config.app.windows.at(0);

    expect(mainWindow?.resizable).toBe(true);
    expect(mainWindow?.minWidth).toBe(1024);
    expect(mainWindow?.minHeight).toBe(720);
  });
});
