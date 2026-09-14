# Experimental save parser worker

This package is intentionally excluded from the Pal Companion Cargo workspace
and Windows alpha package.

The current parser graph includes pinned Git dependencies, a GPL-linked
`ooz-rs` component, and an upstream `psp-core` revision without a complete
license file. It is retained only for local, read-only parser validation while
the corresponding source bundle and license review are incomplete.

Do not add this package back to the workspace, CI release build, or installer
until all of the following are complete:

1. Replace or approve every Git dependency through the supply-chain review.
2. Provide the complete corresponding source required by the GPL component.
3. Resolve the incomplete upstream license material.
4. Re-run the save-parser fixture and corruption-boundary test suites.
5. Update `third_party/palworld-save-parser.lock.json` and the notices.

The production application must report that detailed save parsing is
unavailable when no reviewed parser worker is installed.
