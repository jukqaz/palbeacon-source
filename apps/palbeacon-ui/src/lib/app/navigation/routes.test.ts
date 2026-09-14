import { describe, expect, it } from 'vitest';
import {
  commonPrimaryDestinations,
  isNavigationTargetActive,
  primaryDestinations,
  routeForPath,
  secondaryNavigationFor,
  utilityDestinations,
  visibleRoutes,
  windowsPrimaryDestinations,
} from './routes';

describe('PalBeacon navigation contract', () => {
  it('keeps private Windows destinations out of every Web menu source', () => {
    expect(primaryDestinations('web').map((destination) => destination.label_ko)).toEqual([
      '홈',
      '지도',
      '도감',
      '계획',
    ]);
    expect(utilityDestinations('web').map((destination) => destination.label_ko)).toEqual(['검색']);
    expect(windowsPrimaryDestinations('web')).toEqual([]);
    expect(visibleRoutes('web').some((route) => route.group === 'pc')).toBe(false);
    expect(visibleRoutes('web').some((route) => route.id === 'pc.settings')).toBe(false);
  });

  it('adds the private game destination only for the Windows shell', () => {
    expect(commonPrimaryDestinations('windows').map((destination) => destination.id)).toEqual([
      'home',
      'map',
      'catalog',
      'plan',
    ]);
    expect(windowsPrimaryDestinations('windows')).toHaveLength(1);
    expect(windowsPrimaryDestinations('windows').at(0)).toMatchObject({
      id: 'pc',
      label_ko: '내 게임',
      path: '/connection/profile/',
      visibility: 'desktop',
    });
    expect(utilityDestinations('windows').map((destination) => destination.label_ko)).toEqual([
      '검색',
    ]);
  });

  it('maps direct and grouped routes to one active primary destination', () => {
    const destinations = primaryDestinations('web');
    const activeId = (path: string) =>
      destinations.find((destination) => isNavigationTargetActive(destination, routeForPath(path)))
        ?.id;

    expect(activeId('/items/')).toBe('catalog');
    expect(activeId('/map/')).toBe('map');
    expect(activeId('/plan/materials/')).toBe('plan');
    expect(activeId('/wiki/')).toBeUndefined();
    expect(activeId('/search/')).toBeUndefined();
    expect(activeId('/')).toBe('home');
  });

  it('uses one lightweight horizontal secondary structure for every feature group', () => {
    expect(secondaryNavigationFor('catalog', 'web')?.items.map((item) => item.label_ko)).toEqual([
      '팰',
      '아이템',
      '스킬',
      '기술',
      '건축물',
    ]);
    expect(secondaryNavigationFor('plan', 'web')?.items.map((item) => item.label_ko)).toEqual([
      '팀 구성',
      '교배',
      '팰 비교',
      '재료',
    ]);
    expect(secondaryNavigationFor('pc', 'web')).toBeNull();
    expect(secondaryNavigationFor('pc', 'windows')?.items.map((item) => item.label_ko)).toEqual([
      '내 데이터',
      '서버',
      '오버레이',
      '앱 설정',
    ]);
    expect(routeForPath('/settings/')).toMatchObject({
      id: 'pc.settings',
      group: 'pc',
      availability: ['windows'],
    });
  });

  it('keeps active and passive routes under one skill destination', () => {
    const skill = secondaryNavigationFor('catalog', 'web')?.items.find(
      (destination) => destination.id === 'skills',
    );
    expect(skill).toBeDefined();
    expect(isNavigationTargetActive(skill!, routeForPath('/skills/active/'))).toBe(true);
    expect(isNavigationTargetActive(skill!, routeForPath('/skills/passive/'))).toBe(true);
  });

  it('keeps one purpose per planning menu without legacy routes', () => {
    expect(
      visibleRoutes('web')
        .filter((route) => route.group === 'plan')
        .map((route) => route.label_ko),
    ).toEqual(['팀 구성', '교배 계산', '팰 비교', '재료 계산']);
    expect(routeForPath('/plan/team/')).toBeUndefined();
    expect(routeForPath('/plan/travel/')).toBeUndefined();
    expect(routeForPath('/shops/')).toBeUndefined();
    expect(routeForPath('/wiki/')).toBeUndefined();
    expect(routeForPath('/plan/assistant/')).toBeUndefined();
  });
});
