import type { EntryGenerator } from './$types';

// Cloudflare serves this generated 404.html with an actual 404 status.
export const prerender = true;
export const entries: EntryGenerator = () => [{ path: '404' }];
