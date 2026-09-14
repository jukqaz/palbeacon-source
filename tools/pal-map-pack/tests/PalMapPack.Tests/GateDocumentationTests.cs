using PalMapPack;

namespace PalMapPack.Tests;

public sealed class GateDocumentationTests
{
    private static string WorkspacePath(params string[] parts)
    {
        var root = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "..", "..", "..", "..", "..", "..", ".."));
        return Path.Combine(new[] { root }.Concat(parts).ToArray());
    }

    [Fact]
    public void BuildContractRecordsReviewedExactBuildAndIndependentEvidence()
    {
        var contract = AssetContract.Load(WorkspacePath(
            "tools", "pal-map-pack", "contracts", "24181527.asset-contract.json"));
        Assert.True(contract.Reviewed);
        Assert.Equal(Fixture.Build, contract.GameBuildId);
        Assert.Equal(
            "build-24181527-table-bounds-plus-palworld-save-tools-cbd98c4",
            contract.ReviewId);
        Assert.Equal(
            "241c45de9d5b55b246cd4b39d62b9209faf7758ce0637e1f7a545aa0f75f71f0",
            contract.ApprovedMappingSha256);
        Assert.Equal(
            "d5d28a934bcb5975dea8343a049bc7452616e01c0f6f18d0d6d455ba320f113c",
            contract.ApprovedSealedHoldoutSha256);
        Assert.NotNull(contract.MappingCandidate);
        Assert.Equal("42cf396e", contract.MappingCandidate.Commit);
        Assert.Equal(2_321_933, contract.MappingCandidate.FileSizeBytes);
        Assert.Equal("241c45de9d5b55b246cd4b39d62b9209faf7758ce0637e1f7a545aa0f75f71f0",
            contract.MappingCandidate.Sha256);
        Assert.Equal("candidate_unverified", contract.MappingCandidate.CompatibilityStatus);
        Assert.Equal("Pal/Content/Pal/Maps/MainWorld_5/PL_MainWorld5",
            contract.PoiSources.WorldPackagePath);
        Assert.Equal(31_159, contract.PoiSources.ExpectedWorldExportCount);
        Assert.Equal(152, contract.PoiSources.ExpectedFastTravelCount);
        Assert.Equal(157, contract.PoiSources.ExpectedDungeonCount);
        Assert.Equal(159, contract.PoiSources.ExpectedBossRawCount);
        Assert.Equal(126, contract.PoiSources.ExpectedBossSemanticCount);
        Assert.Equal(11, contract.PoiSources.DungeonPortalExportTypes.Count);
        var evidence = File.ReadAllText(WorkspacePath(
            "tools",
            "pal-map-pack",
            "calibration",
            "24181527.independent-coordinate-evidence.md"));
        Assert.Contains("cbd98c44923b18d8010d94cc1ca10d1657d55c17", evidence);
        Assert.Contains("automated cross-check", evidence);
    }

    [Fact]
    public void CurrentBuildContractRecordsEquivalenceRevalidation()
    {
        var contract = AssetContract.Load(WorkspacePath(
            "tools", "pal-map-pack", "contracts", "24467282.asset-contract.json"));
        Assert.True(contract.Reviewed);
        Assert.Equal("24467282", contract.GameBuildId);
        Assert.Equal(
            "0163fc8809194ed091bc6a9abe403f8a39030c42647e5af74abfd4357b612571",
            contract.ApprovedSealedHoldoutSha256);

        var evidence = File.ReadAllText(WorkspacePath(
            "tools",
            "pal-map-pack",
            "calibration",
            "24467282.independent-coordinate-evidence.md"));
        Assert.Contains("equivalence revalidation", evidence, StringComparison.Ordinal);
        Assert.Contains("432 exact POIs", evidence, StringComparison.Ordinal);
    }

    [Fact]
    public void GateScriptPreservesTheRequiredPipelineAndContainsNoDownloaderOrInjector()
    {
        var text = File.ReadAllText(WorkspacePath("scripts", "run-map-gate.ps1"));
        var positions = new[] { "doctor", "probe-assets", "accept-contract", "stage", "calibrate", "validate", "publish" }
            .Select(command => text.IndexOf($"'{command}'", StringComparison.Ordinal))
            .ToArray();
        Assert.All(positions, position => Assert.True(position >= 0));
        Assert.True(positions.SequenceEqual(positions.Order()));
        Assert.DoesNotContain("Invoke-WebRequest", text, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain("UE4SS", text, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain("download", text, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void RedactedSummaryRecordsTheExactBuildPassWithoutLocalPaths()
    {
        var summary = File.ReadAllText(WorkspacePath("docs", "validation", "gate-b-summary.md"));
        Assert.Contains("PASS", summary, StringComparison.Ordinal);
        Assert.Contains("0.8302px", summary, StringComparison.Ordinal);
        Assert.Contains("c8e904b854f3f68d50288d25eed0d444746f94302930dc9d5fdd1a6f6899b067",
            summary, StringComparison.Ordinal);
        Assert.DoesNotContain("D:\\SteamLibrary", summary, StringComparison.OrdinalIgnoreCase);
    }
}
