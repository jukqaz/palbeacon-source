import { describe, expect, it } from 'vitest';
import { normalizeSeoPath, publicSeoPages, seoForPath } from './seo';

describe('PalBeacon SEO metadata', () => {
  it('keeps the public fan notice discoverable without implying official affiliation', () => {
    expect(seoForPath('/fan-content-notice')).toMatchObject({
      title: '팬 프로젝트 이용 고지 · PalBeacon',
      index: true,
    });
    expect(seoForPath('/fan-content-notice').description).toContain('비공식·비상업');
  });

  it('normalizes trailing slashes and ignores query state', () => {
    expect(normalizeSeoPath('/pals/?query=도로롱')).toBe('/pals');
    expect(seoForPath('/pals/').title).toBe('팰 도감 · PalBeacon');
  });

  it('keeps private and unknown routes out of the index', () => {
    expect(seoForPath('/settings/').index).toBe(false);
    expect(seoForPath('/settings/').title).toBe('페이지를 찾을 수 없음 · PalBeacon');
    expect(seoForPath('/settings/', true).title).toBe('앱 설정 · PalBeacon');
    expect(seoForPath('/connection/profile/', true).title).toBe('내 데이터 · PalBeacon');
    expect(seoForPath('/missing/').index).toBe(false);
  });

  it('publishes unique canonical paths', () => {
    const paths = publicSeoPages.map((page) => page.path);
    expect(new Set(paths).size).toBe(paths.length);
    expect(paths).toContain('/technology');
    expect(paths).not.toContain('/plan/travel');
    expect(paths).not.toContain('/shops');
    expect(paths).not.toContain('/plan/team');
    expect(seoForPath('/plan/compare/').title).toBe('팰 비교 · PalBeacon');
  });
});
