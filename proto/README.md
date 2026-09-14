# Native protocol versions

`palcompanion.v2` is the only generated runtime API. The core, desktop management client,
overlay, and telemetry agent/client must be upgraded together. Native handshakes require protocol
version 2; the named pipe and coordination events use `.v2`. No v1 listener, generated runtime
module, negotiation fallback, or request translation is provided.

`palcompanion/v1/` is the frozen schema contract from main commit
`a8e602a3300ac1901afb3bdf41b57c9cb4f75983`. It remains an input to Buf's unchanged FILE-level
breaking checks, not an executable compatibility layer. Do not repurpose its tags or names.

V2 retires old hotkey field/action 2 and management role 1. Rotation uses field/action 5 and
management uses role 3, so even accidental unversioned decoding cannot reinterpret those values.
The v2 telemetry service follows STANDARD RPC naming without v1's historical lint exceptions.

Content-addressed snapshots, cloud projections, map coordinates, and public HTTP APIs retain their
independent data-schema versions. Their payloads did not change. Existing wire-v1 data golden
vectors remain byte-for-byte identity checks for those unchanged persisted content schemas.

See the [Protobuf tag guidance](https://protobuf.dev/best-practices/dos-donts/) and
[Buf breaking checks](https://buf.build/docs/breaking/).
