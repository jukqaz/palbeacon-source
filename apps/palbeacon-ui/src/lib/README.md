# PalBeacon UI module boundaries

`src/lib` is organized by dependency direction, not by file type.

```text
lib/
├─ app/         application composition: shell, routes, runtime, PWA, SEO
├─ shared/      feature-agnostic UI, formatting, parsing, search, platform adapters
├─ assistant/   game-data assistant feature
├─ catalog/     unified catalog feature and its runtime schema
├─ connection/  Windows-only local connection feature
├─ map/         map presentation and map-owned runtime schema
├─ personal/    selected local-save projection for public features
├─ technology/  technology board feature and its runtime schema
├─ tools/       planning/calculation feature and its runtime schema
└─ wiki/        knowledge feature and its network schema
```

Rules:

1. `shared` does not import `app` or a feature.
2. A feature does not import `app`; routes compose the two.
3. `app` does not own feature data or feature components.
4. Feature-to-feature imports are explicit and limited to real domain reuse.
5. Runtime schemas stay beside their feature. Only the schema parser is shared.
6. Do not add compatibility re-exports for retired paths.
7. Map, overlay, server, and native IPC ownership stays outside UI restructuring.

`architecture.test.ts` enforces the directory and dependency rules.
