# PalBeacon Cloudflare edge

This directory contains the public Cloudflare host for PalBeacon. The browser
UI is built from `apps/palbeacon-ui`; the edge runtime is Rust/Wasm with D1.
Node and pnpm are pinned by the root workspace and build SvelteKit plus the
local Wrangler CLI. Neither is bundled into the Tauri application.

## Security boundary

- Keep `/api/v1/catalog/*`, `/api/v1/knowledge/status`,
  `/api/v1/wiki/{search,read}`, `/api/v1/knowledge/related`, the static web
  application, and its exact-build game asset path public.
  Public responses expose verified dataset metadata, never personal
  projections.
- Cloudflare Access is not part of the deployed product.
- `/api/v1/me/*` and `/api/v1/ops/*` are disabled by default and return `404`.
  Never remove this fail-closed boundary to make personal data public.
- Personal save files, platform IDs, IP addresses, and server credentials never
  enter the public Cloudflare deployment.

## Required configuration

1. Keep the public D1 database named `palbeacon-data` and the private R2 bucket
   named `palbeacon-media`.
2. Run `pnpm d1:migrate:public` from this directory. It applies the eleven D1
   migrations in lexical order and then idempotently reconciles the compound
   triggers in `schema/post_migration_triggers.sql`. Keep the post-migration
   step: Wrangler's remote migration ledger statement cannot share a D1
   `/query` request with compound trigger definitions.
3. Run `mise install`, then `pnpm install --frozen-lockfile` from the repository
   root. The workspace installs the exact Wrangler version.
4. Keep `worker-build` in the repository-managed Rust toolchain at
   `.tools/cargo/bin/worker-build.exe`.
5. Ensure the approved local map tiles exist below
   `assets/palbeacon/game/map/tiles/` and
   `assets/palbeacon/game/map/tree/tiles/`. The public fan
   project currently commits and deploys these files under the disclosure and
   takedown policy in `docs/legal/FAN_CONTENT_NOTICE.ko.md`.

## Public deployment

The committed configuration publishes the browser application and non-personal
dataset through the `palbeacon-web` Worker at <https://palbeacon.jukqaz.xyz>. Its
`workers.dev` endpoint and preview URLs are disabled. Exact-build game assets
are served from the same origin and cached at the edge.

Run, in order:

```text
pnpm --dir cloudflare cf:whoami
pnpm --dir cloudflare d1:migrate:public
pnpm --dir cloudflare cf:check
pnpm --dir cloudflare cf:deploy
```

`cf:check` and `cf:deploy` run Oxfmt, Svelte diagnostics, Oxlint and Vitest,
then build `apps/palbeacon-ui/build` and project the verified public catalog
manifest into that output. Wrangler uploads the verified SvelteKit static
output directly as Workers Static Assets. The catalog projection keeps exact
Korean names, structural numeric fields, typed relations, and breeding rules;
it excludes game artwork, localized descriptions, package paths, and personal
data. The rest of the build contains only the exact data and images selected by
`sync-exact-data.mjs`; separately owned brand media remains on R2.

Every web build also contains `/fan-content-notice`, and the Svelte shell
identifies itself as a non-commercial, unofficial fan project. The notice
prevents official-source confusion but is not represented as a copyright
license.

## Public media on private R2

The `palbeacon-media` bucket has no `r2.dev` or custom-domain public
endpoint. The Worker reads it through the `PUBLIC_MEDIA` binding and exposes
only strict content-addressed requests:

```text
/media/v2/<sha256>/<approved-file-name>
```

`cloudflare/public-media-policy.json` is the sole public-media allowlist. Every
entry must be `public_web_approved`, live below the self-owned generated brand
root, and match its reviewed SHA-256. The projection script fails closed on
game-derived, renamed, modified, or unapproved files.

Build and publish the immutable objects before deploying a Svelte build that
references their routes:

```text
pnpm --dir cloudflare media:build
pnpm --dir cloudflare media:publish
```

The publish script uses mise-managed Node LTS and skips byte-identical objects.
Worker responses stream the R2 body, use a one-year immutable browser policy,
and populate the edge Cache API. Windows continues to read the same brand
files from the neutral `assets/palbeacon` bundle.

Game map tiles, Pal/item/building artwork, wanted portraits, and reviewed POI
icons are deployed through Workers Static Assets rather than this R2 policy.
This policy remains limited to content-addressed brand media.

`wrangler.toml` is the single deployment source for `palbeacon.jukqaz.xyz`; do not create
a parallel Pages project or a second Worker for the same web application. The
inactive local seed is not published or activated by these commands.

## Game update policy

- Public catalog and map requests do not validate a visitor's installed game
  build. `GET /api/v1/catalog/status` reports the active verified dataset as
  provenance only.
- Personal projection imports record previously unseen builds in
  `game_build_policies` and return `BUILD_REVIEW_REQUIRED`.
- `GET /api/v1/ops/game-builds` lists observed builds. An operator can map a
  compatible build to an existing verified dataset with
  `POST /api/v1/ops/game-builds`, or block it. This is a D1 write and does not
  require rebuilding or redeploying the desktop app or Worker.
- Activating a new exact-build verified `full_catalog` dataset automatically
  marks that build compatible through a D1 trigger. Activating a
  `public_metadata` Wiki projection never changes personal-projection
  compatibility.
- Unknown builds never become compatible automatically. Local-only features
  remain available while the personal cloud projection waits for review.

The current local `steam-24181527-v1` seed candidate is SQL-verified but remains
unverified and inactive because its catalog provenance quality is `unknown`.
Cloudflare deployment and D1 migrations may proceed, but public dataset
activation must wait for the exact-build publication review.

## Knowledge Wiki and graph

`pal-cloud-seed` also compiles the exact-build catalog into a deterministic
knowledge graph, reviewed Korean Wiki pages, FTS rows, source-artifact lineage
and an Error Book. Missing relation endpoints are recorded as errors instead of
being replaced by invented nodes.

The knowledge manifest has an independent publication gate. An active dataset
is retrievable only after its knowledge manifest is `validated`, has no
unresolved `open` or `accepted` errors, and is consequently `published`.
Creating a new unresolved error demotes it to `candidate` immediately.

For the public host, generate a rights-safe metadata projection from the
verified local item and world catalogs:

```text
powershell -File scripts/cloudflare/new-public-knowledge-seed.ps1 `
  -OutputDirectory artifacts/cloudflare/public-knowledge-steam-24575825-v1 `
  -DatasetVersion public-meta-steam-24575825-v1 `
  -GameVersion steam-build-24575825 `
  -GameBuildId steam:24575825
```

The compiler includes exact Korean names and structural numeric/relationship
fields, but never copies game descriptions or artwork. It emits ordered,
hash-listed candidate SQL and a separate
`OPERATOR_PROMOTE_AFTER_REVIEW.sql`. Apply only the files listed in
`seed-manifest.json`, verify manifest counts, FTS rows, zero foreign-key errors
and zero open Error Book entries, and then apply the promotion file. The
promotion activates only the `public_metadata` pointer and cannot approve a
game Build for personal sync.

Seed manifest v2 also gives every ordered SQL file a payload hash, conservative
D1 write cost, and an in-transaction checkpoint. A failed file rolls back both
its data and checkpoint. A later run accepts only a contiguous checkpoint
prefix whose path, order, payload hash, and cost still match the local manifest;
an incomplete or changed prefix fails closed.

Always run the cost and idempotency guard before a remote seed:

```text
powershell -File scripts/cloudflare/publish-public-knowledge-seed.ps1 `
  -SeedDirectory artifacts/cloudflare/public-knowledge-steam-24575825-v1
```

The default command is read-only. It verifies every SQL checksum, reads the
active D1 projection, exact remote checkpoints, and the last 24-hour write
usage. A byte-identical active projection always costs zero writes. The guard
uses a conservative 90,000-row daily budget below the Free-plan 100,000-row
limit. When the full snapshot exceeds that budget, `ready_partial` schedules
only the largest contiguous prefix that fits; `wait_for_write_reset` writes
nothing. `-Apply` applies only the scheduled prefix and leaves the previous
public dataset active.

For a multi-day Free-plan publication, use this sequence:

```text
# Day 1: apply only the reported prefix.
powershell -File scripts/cloudflare/publish-public-knowledge-seed.ps1 `
  -SeedDirectory artifacts/cloudflare/public-knowledge-steam-24575825-v1 `
  -Apply

# After the next 00:00 UTC quota reset, rerun the plan. If the conservative
# rolling 24-hour usage still returns wait_for_write_reset, wait and rerun it.
powershell -File scripts/cloudflare/publish-public-knowledge-seed.ps1 `
  -SeedDirectory artifacts/cloudflare/public-knowledge-steam-24575825-v1 `
  -Apply

# Review counts, FTS, foreign keys, and the Error Book. The read-only plan must
# say ready_to_promote before this final, zero-seed-write activation step.
powershell -File scripts/cloudflare/publish-public-knowledge-seed.ps1 `
  -SeedDirectory artifacts/cloudflare/public-knowledge-steam-24575825-v1 `
  -Apply -Promote
```

Never use `-Promote` while pending files remain. Use `-AllowBudgetOverride`
solely for an intentional Paid-plan or separately approved maintenance window.
Cloudflare billing analytics, rather than the estimate, remain authoritative.

Public read-only routes:

- `GET /api/v1/catalog/status`
- `GET /api/v1/knowledge/status`
- `GET /api/v1/wiki/search?query=...&limit=...`
- `GET /api/v1/wiki/read?page_id=...`
- `GET /api/v1/knowledge/related?node_id=...&limit=...`

The public Wiki contracts are `GET`-only so every supported read can use the
same cache key. Legacy `POST` reads are rejected before a D1 binding is opened,
which prevents clients and crawlers from bypassing the edge cache. The removed
public catalog-search API is served from versioned static data instead.
Svelte uses the `GET` contracts so browser HTTP caching, conditional `ETag`
requests and the Workers Cache API can skip repeat D1 reads. Only the anonymous
public-read allowlist is cached:

- status: browser 60 seconds, edge 5 minutes;
- search: browser 5 minutes, edge 15 minutes;
- Wiki documents: browser 1 hour, edge 6 hours;
- graph relations: browser 30 minutes, edge 2 hours.

Responses expose `X-Pal-Cache` as `HIT`, `MISS`, or `BYPASS` for smoke tests.
Cache population runs through `ctx.waitUntil`; cache failures fail open to D1.
When either fixed status route reports a temporary dependency failure, that
`503` is cached at the edge for 30 seconds to prevent a retry storm. Wiki
errors, disabled personal/operator routes, and mutations remain `no-store`.
Status cache keys discard query-string noise, while Wiki requests reject
unknown query fields before executing D1 reads. This prevents arbitrary cache
busters from multiplying equivalent database queries.
Workers Logs use a 5% head sample; billing analytics and explicit smoke-test
headers remain the source of truth for production verification.
Static SvelteKit assets bypass the Worker. Entry points and generated assets
always revalidate, while non-entry assets use bounded browser
`stale-while-revalidate` caching.

### Disabled personal and operator compatibility routes

The database schema and Worker code still retain the previous review contracts
so an offline maintenance tool can be built without losing their validation
logic. The public `palbeacon-web` deployment does not set
`PALBEACON_ENABLE_PERSONAL_CLOUD`, so these routes return `404` before any
identity or database mutation is attempted:

- `GET /api/v1/ops/knowledge/errors`
- `POST /api/v1/ops/knowledge/errors/review`
- `GET /api/v1/ops/knowledge/manifests`
- `POST /api/v1/ops/knowledge/manifests/validate`
- `GET /api/v1/ops/knowledge/wiki-revisions`
- `POST /api/v1/ops/knowledge/wiki-revisions/review`

The three `GET` queues accept `limit` (default `50`, range `1..=100`),
`dataset_version`, `status`, and an opaque `cursor`. Responses retain
`items`/`count` and add nullable `next_cursor`. Error Book defaults to all
statuses, manifests default to `candidate` plus `validated`, and Wiki revisions
default to `draft`. A cursor is tied to its queue and filter values; changing a
filter while reusing it, malformed base64/JSON, or an out-of-range limit
returns `400`. Decoded cursor fields are validated and used only as D1 bind
parameters in stable keyset comparisons.

Every review mutation supplies the row's current `review_version`; successful
updates increment it, while stale decisions return `409 CONFLICT`. Error Book
reviews additionally supply `expected_status`. Manifest validation requires
the expected manifest SHA and verifies the dataset, compiler run, exact or
measured provenance, canonical row counts, reviewed Wiki pages, claims, and
unresolved Error Book entries. It transitions only `candidate` to `validated`;
the existing publication trigger remains the sole route to `published`.

Wiki revision review accepts `approve` or `reject`. Approval atomically marks
the selected revision reviewed, supersedes the previous reviewed revision, and
copies the approved Korean content and exact/measured claims into the public
Wiki tables. Successful, blocked, and stale database review decisions write
redacted operational metadata to `audit_events`; review bodies are limited to
32 KiB and are never stored in that audit table.

The Windows overlay remains local-first. Cloudflare failure must never stop the
overlay renderer, game-window tracking, local catalog, or local analysis.
Windows packages are not built by the Cloudflare pipeline. The Tauri bundle is
built by `pnpm desktop:build`; a public GitHub Release additionally requires a
valid Authenticode signature and a separately reviewed release action.
