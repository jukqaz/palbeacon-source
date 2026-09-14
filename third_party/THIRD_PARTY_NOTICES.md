# Third-Party Notices

Pal Companion does not redistribute user save files. This local private-alpha
package includes a bounded static catalog from the pinned
`palworld-save-pal` revision so that internal species, passive, and active skill
ids can be shown with Korean names and Paldeck numbers. Redistribution remains
blocked pending final content and license review. Rust dependency versions and
checksums are fixed by `Cargo.lock`; JavaScript and NuGet dependency versions
are fixed by `pnpm-lock.yaml` and their committed lockfiles.

`pal-catalog-worker.exe` and `pal-evidence-worker.exe` are local, bounded,
network-free subprocesses. They use the same pinned catalog to produce
human-facing search results and a provenance-labelled evidence bundle. The
evidence worker does not invoke or bundle an AI model.

The machine-readable review inventory is
`third_party/runtime-dependencies.json`. It records ownership, purpose,
native/unsafe boundaries, background execution, and rollback paths.

## Private-alpha game icons

The Windows private-alpha UI includes a limited set of Pal and element icons
under `assets/palbeacon/game`. They were provisioned from the
local Palworld Save Tools 2.2.1 benchmark package and are used only to validate
image-first search, autocomplete, and owned-Pal views. These images depict
Palworld game content and remain property of Pocketpair or the applicable
rightsholders. The open-source license of a community extraction or save tool
does not relicense the extracted game content.

These files must not be treated as generally redistributable application
assets. Public distribution remains blocked until the content policy and
rights review is complete. The production asset pipeline should provision
equivalent icons from a user-owned installation or an explicitly authorized
content package.

## Direct runtime components

- Prost 0.14.4 — Apache-2.0
- Tonic 0.14.6 — MIT
- Tokio 1.53.0 — MIT
- windows-sys 0.61.2 — MIT OR Apache-2.0
- ring 0.17.14 — Apache-2.0 AND ISC
- rusqlite 0.40.1 with bundled SQLite — MIT; SQLite is public domain
- ssh2 0.9.6 — MIT OR Apache-2.0
- wasm-bindgen 0.2.126 — MIT OR Apache-2.0

## Fullscreen overlay runtime

- hudhook 0.9.2 - MIT
- shared_memory 0.12.4 - MIT OR Apache-2.0

`hudhook` provides the pinned DirectX 11/12 Present hooks and explicit
LoadLibrary injection boundary used only for the exact tracked
`Palworld-Win64-Shipping.exe` process. `shared_memory` carries the bounded
pre-rasterized RGBA minimap frame between `pal-overlay.exe` and the injected
renderer; the injected renderer does not receive REST credentials, save
contents, or Cloudflare tokens.

## Native and generated boundaries

`windows-sys` exposes raw Win32 APIs. `ring` includes reviewed C and assembly
cryptography. `rusqlite` is approved only with its pinned bundled SQLite
feature when the local data authority adopts it. `protoc-bin-vendored` is a
build-time executable and is not shipped as a Pal Companion runtime process.
`ssh2` provides the read-only SFTP client boundary and links through
`libssh2-sys`; the product verifies the configured server host-key fingerprint
before password authentication and does not expose SFTP credentials to the
WebView or overlay processes.

The local-only .NET extraction tools use CUE4Parse and CUE4Parse-Conversion
`1.2.2.202607`. They read user-owned installed game files offline and are not
part of the shipped Windows runtime. See the extractor-local NOTICE files for
the reviewed source-contract provenance.

## Development-only map benchmark preparation

`scripts/prepare-overlay-pois.ps1` can explicitly download Pal portraits from
PalCalc revision `922822d99076465e026364f7b07f257f46b3e7a6` for an unverified,
development-only comparison artifact. PalCalc is copyright 2024 Tyler Camp and
licensed under the MIT License; the complete license text is retained as
`third_party/palcalc.LICENSE.txt`. The script is not part of the shipped
runtime, does not provide exact installed-game authority, and refuses network
downloads unless the operator passes its explicit third-party benchmark flag.

## Quarantined save parser subprocess

The experimental `pal-save-parser-worker` source is kept outside the main
Cargo workspace and is not included in the Windows alpha package. It is a
separate read-only subprocess prototype that uses:

- `psp-core` 1.2.0 at
  `d9ccf27a56fbcf6cc538ca37c1e713417a10e925`. Its upstream README declares
  MIT, but that revision contains no complete `LICENSE` file.
- `uesave` 0.7.1 at
  `a5271781df0ed021d72e5ad6eab1c59d5199451c`, licensed MIT.
- `ooz-rs` 0.1.0 at
  `c197be7a4b49b2f37f339888da1c10ab87780c19`, which declares
  GPL-3.0-or-later and includes native submodules.

The exact revisions, hashes, submodules, reviewed modules, and validation
commands are recorded in `third_party/palworld-save-parser.lock.json`.
Building or distributing the parser worker as part of Pal Companion is blocked
until a complete corresponding GPL source bundle and final license review are
provided. The pinned Korean species, passive, and active skill catalogs and
their exact hashes are recorded in the same lock file and are subject to the
same restriction. If the reviewed worker is restored later, the rest of Pal
Companion will communicate with it only through a bounded JSON subprocess
boundary.

This notice summarizes licenses; the corresponding packages contain their
complete license texts and attribution requirements.
