export const PALBEACON_ORIGIN = 'https://palbeacon.jukqaz.xyz';
export const PALBEACON_SOCIAL_IMAGE = '/generated/brand/app-mark-square-v3.png';

export interface SeoMetadata {
  path: string;
  title: string;
  description: string;
  index: boolean;
}

const PUBLIC_PAGES: readonly SeoMetadata[] = [
  {
    path: '/fan-content-notice',
    title: '팬 프로젝트 이용 고지 · PalBeacon',
    description: '팰비콘은 Pocketpair와 제휴하지 않은 비공식·비상업 팰월드 팬 프로젝트입니다.',
    index: true,
  },
  {
    path: '/',
    title: 'PalBeacon · 팰월드 데이터 탐색',
    description: '팰·아이템·기술·스킬·건축물 정보를 한글로 탐색하고 관련 정보를 함께 확인합니다.',
    index: true,
  },
  {
    path: '/search',
    title: '통합 검색 · PalBeacon',
    description: '팰·아이템·스킬·건축물·기술을 한글 표시명과 게임 특징으로 함께 검색합니다.',
    index: true,
  },
  {
    path: '/map',
    title: '탐험 지도 · PalBeacon',
    description: '빠른 이동, 보스, 던전과 지명수배 위치를 검색하는 팰월드 탐험 지도입니다.',
    index: true,
  },
  {
    path: '/pals',
    title: '팰 도감 · PalBeacon',
    description: '팰의 속성·능력치·작업 적성·스킬·드롭을 한 화면에서 탐색합니다.',
    index: true,
  },
  {
    path: '/items',
    title: '아이템 도감 · PalBeacon',
    description: '게임 UI와 동일한 등급, 가격, 제작식과 획득 관계를 확인합니다.',
    index: true,
  },
  {
    path: '/skills/active',
    title: '액티브 스킬 · PalBeacon',
    description: '팰월드 액티브 스킬의 속성·위력·쿨타임·유효 거리를 비교합니다.',
    index: true,
  },
  {
    path: '/skills/passive',
    title: '패시브 스킬 · PalBeacon',
    description: '패시브 스킬 이름과 게임 내 효과를 탐색합니다.',
    index: true,
  },
  {
    path: '/buildings',
    title: '건축물 도감 · PalBeacon',
    description: '건축 분류·내구도·방어·랭크·필요 재료를 탐색합니다.',
    index: true,
  },
  {
    path: '/technology',
    title: '기술 도감 · PalBeacon',
    description: '일반 기술과 고대 기술을 레벨·포인트·해금 아이템 기준으로 나눠 확인합니다.',
    index: true,
  },
  {
    path: '/plan',
    title: '팀 구성 · PalBeacon',
    description: '팰 기본 수치를 사용자가 고른 기준으로 비교해 팀 후보를 구성합니다.',
    index: true,
  },
  {
    path: '/plan/breeding',
    title: '교배 계산기 · PalBeacon',
    description: '팰 데이터로 정방향·역방향 교배와 최단 도달 경로를 계산합니다.',
    index: true,
  },
  {
    path: '/plan/compare',
    title: '팰 비교 · PalBeacon',
    description: '작업 적성·식사량 또는 탑승 질주·스태미나를 전환해 팰을 비교합니다.',
    index: true,
  },
  {
    path: '/plan/materials',
    title: '재료 계산 · PalBeacon',
    description: '제작 목표에 필요한 팰월드 재료 수량과 제작 관계를 계산합니다.',
    index: true,
  },
] as const;

const PRIVATE_PAGES: readonly SeoMetadata[] = [
  {
    path: '/connection/profile',
    title: '내 데이터 · PalBeacon',
    description: 'Windows 앱의 로컬 Palworld 세이브 연결 화면입니다.',
    index: false,
  },
  {
    path: '/connection/servers',
    title: '서버 연결 · PalBeacon',
    description: 'Windows 앱의 개인 서버와 SFTP 연결 화면입니다.',
    index: false,
  },
  {
    path: '/connection/overlay',
    title: '오버레이 설정 · PalBeacon',
    description: 'Windows 앱의 로컬 게임 오버레이 설정 화면입니다.',
    index: false,
  },
  {
    path: '/settings',
    title: '앱 설정 · PalBeacon',
    description: 'PalBeacon Windows 앱의 로컬 설정 화면입니다.',
    index: false,
  },
] as const;

const publicPages = new Map(PUBLIC_PAGES.map((metadata) => [metadata.path, metadata]));
const privatePages = new Map(PRIVATE_PAGES.map((metadata) => [metadata.path, metadata]));

export const normalizeSeoPath = (path: string): string => {
  const withoutQuery = path.split(/[?#]/u, 1)[0] ?? '/';
  if (withoutQuery === '/') return '/';
  return `/${withoutQuery.replaceAll(/^\/+|\/+$/gu, '')}`;
};

export const seoForPath = (path: string, includePrivate = false): SeoMetadata => {
  const normalized = normalizeSeoPath(path);
  return (
    publicPages.get(normalized) ??
    (includePrivate ? privatePages.get(normalized) : undefined) ?? {
      path: normalized,
      title: '페이지를 찾을 수 없음 · PalBeacon',
      description: 'PalBeacon에 등록되지 않은 경로입니다.',
      index: false,
    }
  );
};

export const publicSeoPages = PUBLIC_PAGES;
