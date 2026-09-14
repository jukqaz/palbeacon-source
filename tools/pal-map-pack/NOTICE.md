# Notices

`pal-map-pack` is an offline local extractor. It does not contain or redistribute
Palworld assets, mappings, or runtime binaries.

The coordinate-validation workflow was initially informed by the MIT-licensed PalCalc project and
its `PalCalc.GenDB` coordinate-sample workflow:

- Project: https://github.com/tylercamp/palcalc
- License: MIT
- Local changes: the production matrix is derived only from the exact-Build world-map bounds and
  decoded texture dimensions. Evidence does not fit or alter the matrix; exactly ten reference
  points and five precommitted values must independently cover five zones and pass residual
  thresholds. The original Build 24181527 review uses a separately implemented
  PalworldSaveTools coordinate conversion as the automated cross-check; its
  pinned source commit and constants are recorded beside the evidence file.
  Build 24467282 reuses those observations only after proving that the decoded
  map assets and coordinate-defining bounds are unchanged.
  Deterministic 25-point parity evidence validates the serialized transform in both .NET and Rust.

No PalCalc map image, JPEG preview, coordinate file, or other game-derived artifact is included.
The PalCalc preview transform is not production compatibility evidence.

CUE4Parse and CUE4Parse-Conversion are pinned to `1.2.2.202607` under their respective upstream
licenses. The local adapter uses that pinned API to probe exact installed Builds, but candidate probe
evidence is not contract approval or Gate B GO. It never calls a mapping/native-decoder download
helper and continues to fail closed until the candidate mapping, contract, POI accounting, and live
calibration are reviewed.

SixLabors.ImageSharp is pinned to `3.1.12`. See the NuGet lockfiles for the complete dependency
closure and upstream license metadata.
