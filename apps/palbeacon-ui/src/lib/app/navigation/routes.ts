import { resolve } from '$app/paths';
import routesDocument from '$contracts/routes.v1.json';
import type { PalPlatform } from '$lib/app/platform/runtime';

export type RouteGroup = 'explore' | 'catalog' | 'plan' | 'pc';
type NavigationVisibility = 'all' | 'compact' | 'desktop';
type NavigationScope = 'common' | 'windows';

export interface PalRoute {
  id: string;
  path: string;
  group: RouteGroup;
  label_ko: string;
  availability: PalPlatform[];
  navigation?: boolean;
}

interface NavigationTarget {
  id: string;
  label_ko: string;
  path: string;
  availability?: PalPlatform[];
  active_group?: RouteGroup;
  active_route_ids?: string[];
}

export interface PrimaryDestination extends NavigationTarget {
  scope: NavigationScope;
  visibility: NavigationVisibility;
}

export type UtilityDestination = NavigationTarget;
export type SecondaryDestination = NavigationTarget;

interface SecondaryNavigationGroup {
  group: RouteGroup;
  label_ko: string;
  items: SecondaryDestination[];
}

interface RouteContract {
  routes: PalRoute[];
  navigation: {
    primary: PrimaryDestination[];
    utilities: UtilityDestination[];
    secondary: SecondaryNavigationGroup[];
  };
}

const contract = routesDocument as RouteContract;
const routes = contract.routes;

const availableOn = (target: NavigationTarget, platform: PalPlatform): boolean =>
  target.availability?.includes(platform) ?? true;

export const isNavigationTargetActive = (
  target: NavigationTarget,
  route: PalRoute | undefined,
): boolean =>
  route !== undefined &&
  (target.active_group === route.group || target.active_route_ids?.includes(route.id) === true);

export const primaryDestinations = (platform: PalPlatform): readonly PrimaryDestination[] =>
  contract.navigation.primary.filter((destination) => availableOn(destination, platform));

export const commonPrimaryDestinations = (platform: PalPlatform): readonly PrimaryDestination[] =>
  primaryDestinations(platform).filter((destination) => destination.scope === 'common');

export const windowsPrimaryDestinations = (platform: PalPlatform): readonly PrimaryDestination[] =>
  primaryDestinations(platform).filter((destination) => destination.scope === 'windows');

export const utilityDestinations = (platform: PalPlatform): readonly UtilityDestination[] =>
  contract.navigation.utilities.filter((destination) => availableOn(destination, platform));

export const secondaryNavigationFor = (
  group: RouteGroup,
  platform: PalPlatform,
): { label: string; items: readonly SecondaryDestination[] } | null => {
  const navigation = contract.navigation.secondary.find((entry) => entry.group === group);
  if (!navigation) return null;
  const items = navigation.items.filter(
    (destination) =>
      availableOn(destination, platform) &&
      destination.active_route_ids?.some((routeId) =>
        routes.some((route) => route.id === routeId && route.availability.includes(platform)),
      ) === true,
  );
  return items.length > 0 ? { label: navigation.label_ko, items } : null;
};

export const visibleRoutes = (platform: PalPlatform): PalRoute[] =>
  routes.filter((route) => route.availability.includes(platform) && route.navigation !== false);

export const routeForPath = (path: string): PalRoute | undefined => {
  const normalized = path.endsWith('/') ? path : `${path}/`;
  return routes.find((route) => route.path === normalized || (path === '/' && route.path === '/'));
};

export const resolvePalPath = (path: string): string => {
  if (path === '/') return resolve('/', {});
  if (path === '/technology/') return resolve('/technology/', {});
  return resolve('/[...path]', { path: path.replaceAll(/^\/+|\/+$/g, '') });
};
