import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

const offlineGameData = new Set([
  'generated/game/catalog.v1.json',
  'generated/game/technology.v1.json',
  'generated/game/tools.v1.json',
  'generated/game/work-suitability/manifest.json',
]);

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    adapter: adapter({
      precompress: true,
      strict: true,
    }),
    serviceWorker: {
      register: false,
      files: (file) =>
        !file.startsWith('generated/') ||
        file.startsWith('generated/fonts/') ||
        file.startsWith('generated/brand/') ||
        file.startsWith('generated/map/') ||
        file.startsWith('generated/game/pals/') ||
        file.startsWith('generated/game/elements/') ||
        file.startsWith('generated/game/work-suitability/') ||
        offlineGameData.has(file),
    },
    alias: {
      $contracts: '../../contracts/palbeacon',
    },
  },
};

export default config;
