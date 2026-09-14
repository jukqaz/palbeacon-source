import { PALBEACON_ORIGIN } from '$lib/app/seo/seo';
import type { RequestHandler } from './$types';

export const prerender = true;

export const GET: RequestHandler = () =>
  new Response(
    [
      'User-agent: *',
      'Allow: /',
      'Disallow: /connection/',
      'Disallow: /settings',
      `Sitemap: ${PALBEACON_ORIGIN}/sitemap.xml`,
      '',
    ].join('\n'),
    { headers: { 'content-type': 'text/plain; charset=utf-8' } },
  );
