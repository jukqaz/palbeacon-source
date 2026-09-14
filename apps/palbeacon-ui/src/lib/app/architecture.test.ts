import { readFile, readdir } from 'node:fs/promises';
import { relative, resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const libRoot = resolve(process.cwd(), 'src/lib');
const featureRoots = [
  'assistant',
  'catalog',
  'connection',
  'map',
  'personal',
  'technology',
  'tools',
  'wiki',
];
const canonicalRoots = new Set(['app', 'shared', ...featureRoots]);
const featureImport = new RegExp(`\\$lib/(?:${featureRoots.join('|')})/`, 'u');
const appImport = /\$lib\/app\//u;

const walk = async (directory: string): Promise<string[]> => {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = await Promise.all(
    entries.map(async (entry) => {
      const path = resolve(directory, entry.name);
      return entry.isDirectory() ? walk(path) : [path];
    }),
  );
  return files.flat();
};

const isProductionSource = (path: string): boolean =>
  /\.(?:svelte|ts)$/u.test(path) &&
  !/\.(?:test|stories)\.ts$/u.test(path) &&
  !path.endsWith('.d.ts');

describe('PalBeacon UI architecture', () => {
  it('keeps every lib source inside an app, shared, or feature boundary', async () => {
    const files = (await walk(libRoot)).filter((path) => /\.(?:svelte|ts)$/u.test(path));
    const misplaced = files
      .map((path) => relative(libRoot, path).replaceAll('\\', '/'))
      .filter((path) => !canonicalRoots.has(path.split('/', 1)[0] ?? ''));

    expect(misplaced).toEqual([]);
  });

  it('prevents shared and feature code from depending on app composition', async () => {
    const files = (await walk(libRoot)).filter(isProductionSource);
    const sources = await Promise.all(files.map((path) => readFile(path, 'utf8')));
    const violations: string[] = [];

    for (const [index, path] of files.entries()) {
      const source = sources[index] ?? '';
      const modulePath = relative(libRoot, path).replaceAll('\\', '/');
      if (
        modulePath.startsWith('shared/') &&
        (appImport.test(source) || featureImport.test(source))
      ) {
        violations.push(modulePath);
      }
      if (
        featureRoots.some((root) => modulePath.startsWith(`${root}/`)) &&
        appImport.test(source)
      ) {
        violations.push(modulePath);
      }
      if (modulePath.startsWith('app/') && featureImport.test(source)) violations.push(modulePath);
    }

    expect(violations).toEqual([]);
  });

  it('keeps runtime schemas with their owning features', async () => {
    const files = (await walk(libRoot)).map((path) =>
      relative(libRoot, path).replaceAll('\\', '/'),
    );
    const fileSet = new Set(files);

    expect(fileSet.has('data/runtime-validation.ts')).toBe(false);
    expect(files).toEqual(
      expect.arrayContaining([
        'catalog/runtime-schema.ts',
        'map/runtime-schema.ts',
        'technology/runtime-schema.ts',
        'tools/runtime-schema.ts',
        'shared/data/parse-runtime-payload.ts',
      ]),
    );
  });
});
