# PalBeacon Web UI

PalBeacon의 기본 UI 구현이다. SvelteKit 정적 산출물을 Web에 배포하고 같은 소스를 Tauri
Windows 셸에 로컬 번들한다.

## 로컬 게이트

저장소 루트의 Node `24.19.x`, pnpm `11.19.x`를 사용한다.

```powershell
pnpm install --frozen-lockfile
pnpm ui:format:check
pnpm ui:check
pnpm ui:lint
pnpm ui:test
pnpm ui:build
pnpm ui:test:e2e
pnpm cutover:verify -RequireBuiltWeb
```

`VITE_PALBEACON_API_ORIGIN`은 명시적 API origin이 필요한 preview에서만 사용한다. 설정하지
않으면 운영 Web은 same-origin을, localhost와 Tauri는 공개 PalBeacon API를 사용한다.

제거한 Flutter 앱과 Widgetbook은 Git 기준점과 정적 디자인 문서에서만 참조한다.
전환 정책은 `docs/architecture/ADR-0013-palbeacon-release-cutover.md`를 따른다.
