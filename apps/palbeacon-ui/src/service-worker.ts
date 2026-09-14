/// <reference no-default-lib="true" />
/// <reference lib="esnext" />
/// <reference lib="webworker" />
/// <reference types="@sveltejs/kit" />

import { build, files, prerendered, version } from '$service-worker';
import { precacheInBatches, publicPrecachePages } from './lib/app/pwa/precache';

const worker = globalThis as unknown as ServiceWorkerGlobalScope;
const PRECACHE = `palbeacon-precache-${version}`;
const RUNTIME = `palbeacon-runtime-${version}`;
const essentialStaticFiles = new Set([
  '/manifest.webmanifest',
  '/favicon.png',
  '/generated/brand/app-mark-square-v3.png',
  '/generated/game/catalog.v1.json',
  '/generated/game/technology.v1.json',
  '/generated/game/tools.v1.json',
]);
const offlineCatalogMediaPrefixes = [
  '/generated/game/pals/',
  '/generated/game/elements/',
  '/generated/game/work-suitability/',
];
const isOfflineCatalogAsset = (path: string): boolean =>
  essentialStaticFiles.has(path) ||
  offlineCatalogMediaPrefixes.some((prefix) => path.startsWith(prefix));
const precacheAssets = [
  ...new Set([
    ...build,
    ...publicPrecachePages(prerendered),
    ...files.filter(isOfflineCatalogAsset),
  ]),
];
let lastNetworkAvailable = true;

const isCacheable = (response: Response): boolean =>
  response.ok && !response.headers.get('cache-control')?.includes('no-store');

worker.addEventListener('install', (event) => {
  event.waitUntil(
    caches
      .open(PRECACHE)
      .then((cache) => precacheInBatches(cache, precacheAssets))
      .catch(async (cause: unknown) => {
        await caches.delete(PRECACHE);
        throw cause;
      }),
  );
});

worker.addEventListener('activate', (event) => {
  event.waitUntil(
    (async () => {
      const cacheKeys = await caches.keys();
      await Promise.all(
        cacheKeys
          .filter((key) => key.startsWith('palbeacon-') && key !== PRECACHE && key !== RUNTIME)
          .map((key) => caches.delete(key)),
      );
      await worker.clients.claim();
    })(),
  );
});

worker.addEventListener('message', (event) => {
  const type = (event.data as { type?: string } | null)?.type;
  if (type === 'SKIP_WAITING') {
    void worker.skipWaiting();
    return;
  }
  if (type === 'GET_NETWORK_STATUS' && event.source && 'postMessage' in event.source) {
    event.source.postMessage({ type: 'NETWORK_STATUS', online: lastNetworkAvailable }, []);
  }
});

const networkFirst = async (request: Request): Promise<Response> => {
  const cache = await caches.open(RUNTIME);
  try {
    const response = await fetch(request);
    lastNetworkAvailable = true;
    if (isCacheable(response)) await cache.put(request, response.clone());
    return response;
  } catch (error) {
    lastNetworkAvailable = false;
    const cached = await caches.match(request, { ignoreSearch: true });
    if (cached) return cached;
    const home = await caches.match('/');
    if (home) return home;
    throw error;
  }
};

const cacheFirst = async (request: Request): Promise<Response> => {
  const cached = await caches.match(request);
  if (cached) return cached;
  try {
    const response = await fetch(request);
    lastNetworkAvailable = true;
    if (isCacheable(response)) {
      const cache = await caches.open(RUNTIME);
      await cache.put(request, response.clone());
    }
    return response;
  } catch (error) {
    lastNetworkAvailable = false;
    throw error;
  }
};

worker.addEventListener('fetch', (event) => {
  const { request } = event;
  if (request.method !== 'GET') return;

  const url = new URL(request.url);
  if (url.origin !== worker.location.origin || url.pathname.startsWith('/api/')) return;

  if (request.mode === 'navigate') {
    event.respondWith(networkFirst(request));
    return;
  }

  if (
    precacheAssets.includes(url.pathname) ||
    url.pathname.startsWith('/_app/immutable/') ||
    url.pathname.startsWith('/generated/')
  ) {
    event.respondWith(cacheFirst(request));
  }
});
