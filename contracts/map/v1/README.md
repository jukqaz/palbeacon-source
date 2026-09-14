# Map contract v1

The authoritative Korean POI taxonomy is
`assets/palbeacon/game/map/poi-terminology.ko.v1.json` and the
exact-build POI source is `pois.v1.json` beside it.

`pal-map-contract` validates both sources and generates the manifest and Korean
search index. Generated outputs must never be edited by hand.

```powershell
cargo run -p pal-map-contract --bin generate-map-contract --locked -- --check
cargo run -p pal-map-contract --bin generate-map-contract --locked -- --write
```
