import routesDocument from '$contracts/routes.v1.json';

const publicPages = new Set(
  routesDocument.routes
    .filter((route) => route.availability.includes('web'))
    .map((route) => route.path.replace(/\/$/, '') || '/'),
);

// Prerendering also produces PC screens and an error document. Production
// intentionally returns 404 for them, which must not abort PWA installation.
export const publicPrecachePages = (paths: readonly string[]): string[] =>
  paths.filter((path) => publicPages.has(path.replace(/\/$/, '') || '/'));

// Leave network capacity for the page while installing the offline catalog.
// Queueing every image at once can delay foreground navigation on first visit.
const PRECACHE_BATCH_SIZE = 4;

export const precacheInBatches = async (
  cache: Pick<Cache, 'addAll'>,
  assets: readonly string[],
): Promise<void> => {
  for (let offset = 0; offset < assets.length; offset += PRECACHE_BATCH_SIZE) {
    // oxlint-disable-next-line no-await-in-loop -- Serial batches enforce the download concurrency limit.
    await cache.addAll(assets.slice(offset, offset + PRECACHE_BATCH_SIZE));
  }
};
