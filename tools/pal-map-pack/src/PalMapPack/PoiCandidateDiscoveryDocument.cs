namespace PalMapPack;

public sealed record PoiCandidateDiscoveryDocument(
    int SchemaVersion,
    string GameBuildId,
    int CandidateCount,
    IReadOnlyList<string> PackagePaths);
