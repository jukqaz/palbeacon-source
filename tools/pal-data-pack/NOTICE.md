# Notices

`pal-data-pack` is an offline, local-only extractor. It does not contain or
redistribute Palworld assets, mappings, or save files.

CUE4Parse and CUE4Parse-Conversion are pinned to `1.2.2.202607` under their
upstream licenses. The adapter mounts only the user's installed game files and
uses a user-supplied mapping whose SHA-256 must match the exact-Build contract.
It contains no mapping downloader and performs no runtime network request.

The Build 24181527 game-icon contract exports only from the user's installed
game. Original-resolution PNGs and derived thumbnails are marked
`local_windows_only`; this repository stores their package paths and hashes,
not the extracted game images. Public Web/PWA redistribution requires a
separate rights decision.

The logical table paths and field names in the Build 24181527 candidate
contract were independently verified against the installed game and
cross-checked with the MIT-licensed PalCalc `PalCalc.GenDB` readers:

- Project: https://github.com/tylercamp/palcalc
- Reviewed revision: `59d70fecd99698021809b09760fa0a57adaefea2`
- License: MIT

No PalCalc code, generated database, icon, map, localization, or other
game-derived artifact is distributed by this project. Candidate probe evidence
does not make a contract reviewed or authorize redistribution.

The MIT-licensed PalworldDataTools `PalworldDataExtractor` was also reviewed as
an upstream replacement candidate:

- Project: https://github.com/PalworldDataTools/PalworldDataExtractor
- Reviewed revision: `342d072157f2f15470f27a9e4d6a55490e6c190e`
- License: MIT

It uses CUE4Parse `1.1.1` and covers Pal parameters, character icons, special
breeding, and localization, but it does not cover item icons, equipment model
rendering, or this project's exact-Build contracts. No source code, package, or
game-derived artifact from that project was imported.
