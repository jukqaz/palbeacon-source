# PalBeacon Windows Desktop

공용 SvelteKit UI를 로컬 정적 자산으로 번들하는 Tauri 2 Windows 셸이다. 개인 데이터,
서버 프로필, 설정과 오버레이 제어는 `src-tauri`의 명시적 command allowlist만 통과한다.

```powershell
pnpm install --frozen-lockfile
pnpm desktop:check
pnpm desktop:test
pnpm desktop:build
pnpm cutover:verify -RequireBuiltWeb -RequireWindowsBundle
```

공개 설치 프로그램은 `-RequireSignedWindowsBundle` 게이트도 통과해야 한다. 서명 키와 승인된
업데이트 endpoint가 준비되기 전에는 updater를 켜거나 임시 키로 우회하지 않는다.
