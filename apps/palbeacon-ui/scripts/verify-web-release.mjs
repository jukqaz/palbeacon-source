import { readFile, readdir, stat } from 'node:fs/promises';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const rootDirectory = fileURLToPath(new URL('../', import.meta.url));
const buildDirectory = join(rootDirectory, 'build');

const requiredFiles = [
  '_headers',
  '404.html',
  'index.html',
  'fan-content-notice.html',
  'manifest.webmanifest',
  'pwa/icon-192.png',
  'pwa/icon-512.png',
  'robots.txt',
  'service-worker.js',
  'sitemap.xml',
];

const privatePages = new Set([
  'connection/overlay.html',
  'connection/profile.html',
  'connection/servers.html',
  'settings.html',
]);

const publicPaths = [
  '/',
  '/fan-content-notice',
  '/buildings',
  '/items',
  '/map',
  '/pals',
  '/plan',
  '/plan/breeding',
  '/plan/compare',
  '/plan/materials',
  '/search',
  '/skills/active',
  '/skills/passive',
  '/technology',
];

const failures = [];
const assert = (condition, message) => {
  if (!condition) failures.push(message);
};

const text = async (path) => readFile(join(buildDirectory, path), 'utf8');

for (const path of requiredFiles) {
  try {
    const details = await stat(join(buildDirectory, path));
    assert(details.isFile() && details.size > 0, `${path}: missing or empty`);
  } catch {
    failures.push(`${path}: missing`);
  }
}

const htmlFiles = [];
const clientJavaScriptFiles = [];
const visit = async (directory) => {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) await visit(path);
    else if (entry.name.endsWith('.html')) htmlFiles.push(path);
    else if (
      entry.name.endsWith('.js') &&
      relative(buildDirectory, path).replaceAll('\\', '/').startsWith('_app/immutable/')
    ) {
      clientJavaScriptFiles.push(path);
    }
  }
};
await visit(buildDirectory);

const runtimeNamespacePattern = /__sveltekit_([a-z0-9]+)\b/gu;
const htmlRuntimeNamespaces = new Set();
const clientRuntimeNamespaces = new Set();

for (const path of htmlFiles) {
  const markup = await readFile(path, 'utf8');
  const name = relative(buildDirectory, path).replaceAll('\\', '/');
  const count = (pattern) => [...markup.matchAll(pattern)].length;
  assert(count(/<title>/gu) === 1, `${name}: expected one title`);
  assert(count(/<meta name="description"/gu) === 1, `${name}: expected one description`);
  assert(count(/<link rel="canonical"/gu) === 1, `${name}: expected one canonical URL`);
  assert(count(/<meta property="og:title"/gu) === 1, `${name}: expected Open Graph metadata`);
  assert(count(/<h1(?:\s|>)/gu) >= 1, `${name}: expected rendered h1 content`);
  assert(!markup.includes('<body></body>'), `${name}: emitted an empty app shell`);
  for (const match of markup.matchAll(runtimeNamespacePattern)) {
    htmlRuntimeNamespaces.add(match[1]);
  }

  const shouldBePrivate = name === '404.html' || privatePages.has(name);
  assert(
    markup.includes(shouldBePrivate ? 'noindex, nofollow' : 'index, follow'),
    `${name}: robots policy does not match route visibility`,
  );
}

for (const path of clientJavaScriptFiles) {
  const source = await readFile(path, 'utf8');
  for (const match of source.matchAll(runtimeNamespacePattern)) {
    clientRuntimeNamespaces.add(match[1]);
  }
}

assert(
  htmlRuntimeNamespaces.size === 1,
  `runtime: expected one HTML namespace, found ${[...htmlRuntimeNamespaces].join(', ') || 'none'}`,
);
assert(
  clientRuntimeNamespaces.size === 1,
  `runtime: expected one client namespace, found ${[...clientRuntimeNamespaces].join(', ') || 'none'}`,
);
if (htmlRuntimeNamespaces.size === 1 && clientRuntimeNamespaces.size === 1) {
  assert(
    htmlRuntimeNamespaces.values().next().value === clientRuntimeNamespaces.values().next().value,
    'runtime: prerendered HTML and client chunks were produced by different SvelteKit builds',
  );
}

const manifest = JSON.parse(await text('manifest.webmanifest'));
const fanNotice = await text('fan-content-notice.html');
assert(
  fanNotice.includes('비공식·비상업'),
  'fan notice: unofficial non-commercial disclosure missing',
);
assert(fanNotice.includes('Pocketpair'), 'fan notice: rights holder missing');
assert(manifest.display === 'standalone', 'manifest: display must be standalone');
assert(manifest.start_url === '/', 'manifest: start_url must be root');
assert(manifest.scope === '/', 'manifest: scope must be root');
assert(
  manifest.icons?.some((icon) => icon.sizes === '192x192'),
  'manifest: 192 icon missing',
);
assert(
  manifest.icons?.some((icon) => icon.sizes === '512x512'),
  'manifest: 512 icon missing',
);

const robots = await text('robots.txt');
assert(
  robots.includes('Sitemap: https://palbeacon.jukqaz.xyz/sitemap.xml'),
  'robots: sitemap missing',
);
assert(
  robots.includes('Disallow: /connection/'),
  'robots: private connection routes are indexable',
);
assert(robots.includes('Disallow: /settings'), 'robots: settings route is indexable');

const sitemap = await text('sitemap.xml');
for (const path of publicPaths) {
  const url = `https://palbeacon.jukqaz.xyz${path}`;
  assert(sitemap.includes(`<loc>${url}</loc>`), `sitemap: ${path} missing`);
}
assert(!sitemap.includes('/connection/'), 'sitemap: connection route leaked');
assert(!sitemap.includes('/settings'), 'sitemap: settings route leaked');

const serviceWorker = await text('service-worker.js');
assert(serviceWorker.includes('palbeacon-precache-'), 'service worker: versioned precache missing');
assert(serviceWorker.includes('palbeacon-runtime-'), 'service worker: runtime cache missing');
assert(serviceWorker.includes('GET_NETWORK_STATUS'), 'service worker: connectivity state missing');
assert(serviceWorker.includes('SKIP_WAITING'), 'service worker: update activation missing');
for (const asset of [
  '/generated/game/catalog.v1.json',
  '/generated/game/technology.v1.json',
  '/generated/game/tools.v1.json',
  '/generated/game/pals/T_PinkRabbit_Grass_icon_normal.webp',
  '/generated/game/elements/T_Icon_element_s_04.webp',
  '/generated/game/work-suitability/Handcraft.png',
]) {
  assert(serviceWorker.includes(asset), `service worker: offline data ${asset} missing`);
}
assert(
  serviceWorker.includes('/generated/'),
  'service worker: generated data and media must be cached on first use',
);

if (failures.length > 0) {
  console.error(`Web release verification failed (${failures.length}):`);
  for (const failure of failures) console.error(`- ${failure}`);
  process.exitCode = 1;
} else {
  console.log(`Web release verification passed (${htmlFiles.length} rendered pages).`);
}
