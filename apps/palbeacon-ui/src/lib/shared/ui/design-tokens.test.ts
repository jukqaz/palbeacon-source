import { readdir, readFile } from 'node:fs/promises';
import { extname, join } from 'node:path';
import { describe, expect, it } from 'vitest';

const sourceRoot = join(process.cwd(), 'src');
const supportedExtensions = new Set(['.css', '.svelte']);

const collectStyleSources = async (directory: string): Promise<string[]> => {
  const entries = await readdir(directory, { withFileTypes: true });
  const nested = await Promise.all(
    entries.map(async (entry) => {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) return collectStyleSources(path);
      return supportedExtensions.has(extname(entry.name)) ? [path] : [];
    }),
  );
  return nested.flat();
};

describe('design token contract', () => {
  it('does not reference undefined CSS custom properties', async () => {
    const paths = await collectStyleSources(sourceRoot);
    const sources = await Promise.all(paths.map(async (path) => readFile(path, 'utf8')));
    const declarations = new Set(
      sources.flatMap((source) => [...source.matchAll(/(--[\w-]+)\s*:/g)].map((match) => match[1])),
    );
    const usages = new Set(
      sources.flatMap((source) =>
        [...source.matchAll(/var\((--[\w-]+)/g)].map((match) => match[1]),
      ),
    );

    expect([...usages].filter((token) => !declarations.has(token)).toSorted()).toEqual([]);
  });
});
