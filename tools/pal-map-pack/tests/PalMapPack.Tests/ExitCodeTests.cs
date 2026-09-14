using PalMapPack;

namespace PalMapPack.Tests;

public sealed class ExitCodeTests
{
    [Fact]
    public void GateExitCodesAreStable()
    {
        Assert.Equal(12, ExitCodes.MissingMapping);
        Assert.Equal(13, ExitCodes.MountSerialization);
        Assert.Equal(14, ExitCodes.AssetContractMismatch);
        Assert.Equal(15, ExitCodes.CalibrationRejected);
        Assert.Equal(16, ExitCodes.PackIntegrity);
    }

    [Fact]
    public void DoctorFailsClosedWhenMappingIsAbsent()
    {
        var environment = new Dictionary<string, string?>();
        var result = CommandRunner.Run(new[] { "doctor" }, environment);
        Assert.Equal(ExitCodes.MissingMapping, result);
    }

    [Fact]
    public void ProbeFailsBeforeMountWhenContractIsNotReviewed()
    {
        var contract = Fixture.ApprovedContract() with { Reviewed = false, ReviewId = "candidate-only" };
        var failure = Assert.Throws<MapPackFailure>(() =>
            AssetCatalogProbe.EnsureContractAccepted(
                contract,
                Fixture.Build,
                new AssetInventory(Array.Empty<AssetInventoryEntry>())));
        Assert.Equal(ExitCodes.AssetContractMismatch, failure.ExitCode);
    }

    [Fact]
    public void ReviewedContractRequiresASealedHoldoutCommitment()
    {
        var contract = Fixture.ApprovedContract() with
        {
            ApprovedSealedHoldoutSha256 = null,
        };
        var failure = Assert.Throws<MapPackFailure>(() =>
            AssetCatalogProbe.EnsureContractAccepted(
                contract,
                Fixture.Build,
                Fixture.Inventory()));
        Assert.Equal(ExitCodes.AssetContractMismatch, failure.ExitCode);
        Assert.Contains("holdout", failure.Message, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void ReviewedContractRejectsInvalidAuthoritativeMapDimensions()
    {
        var contract = Fixture.ApprovedContract() with
        {
            MapRegions = Fixture.ApprovedContract().MapRegions.Select(region =>
                region.MapId == "MainMap"
                    ? region with { MapWidthPx = 0 }
                    : region).ToArray(),
        };
        var failure = Assert.Throws<MapPackFailure>(() =>
            AssetCatalogProbe.EnsureContractAccepted(
                contract,
                Fixture.Build,
                Fixture.Inventory()));
        Assert.Equal(ExitCodes.AssetContractMismatch, failure.ExitCode);
        Assert.Contains("map region", failure.Message, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void Cue4ParseAdapterRequiresAnApprovedMappingAndCompleteContainers()
    {
        var reader = new Cue4ParseMapAssetReader();
        var failure = Assert.Throws<MapPackFailure>(() => reader.Read(
            new AssetReadRequest(Fixture.Build, "fixture.usmap", Fixture.ApprovedContract(), Array.Empty<string>())));
        Assert.Equal(ExitCodes.AssetContractMismatch, failure.ExitCode);
        Assert.Contains("approved mapping", failure.Message, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void AcceptContractOnlyVerifiesAnAlreadyReviewedExactBuildContract()
    {
        var root = Fixture.TempDirectory();
        var mapping = Path.Combine(root, "fixture.usmap");
        var contractPath = Path.Combine(root, "contract.json");
        File.WriteAllBytes(mapping, "mapping"u8.ToArray());
        File.WriteAllBytes(contractPath, ManifestWriter.SerializeBytes(Fixture.ApprovedContract()));
        var steamApps = Path.Combine(root, "steamapps");
        var paks = Path.Combine(
            steamApps,
            "common",
            "Palworld",
            "Pal",
            "Content",
            "Paks");
        Directory.CreateDirectory(paks);
        File.WriteAllBytes(Path.Combine(paks, "container.pak"), [1]);
        File.WriteAllText(
            Path.Combine(steamApps, "appmanifest_1623730.acf"),
            "\"AppState\" { \"appid\" \"1623730\" \"installdir\" \"Palworld\" \"buildid\" \"24181527\" }");
        var result = CommandRunner.Run(
            new[]
            {
                "accept-contract",
                "--mappings", mapping,
                "--contract", contractPath,
                "--build", Fixture.Build,
                "--steam-root", root,
            },
            new Dictionary<string, string?>());
        Assert.Equal(ExitCodes.Success, result);
    }

    [Fact]
    public void DoctorRequiresTheExactInstalledSteamBuildAndContainerSet()
    {
        var root = Fixture.TempDirectory();
        var mapping = Path.Combine(root, "fixture.usmap");
        File.WriteAllBytes(mapping, [1]);
        var steamApps = Path.Combine(root, "steamapps");
        var install = Path.Combine(steamApps, "common", "Palworld");
        var paks = Path.Combine(install, "Pal", "Content", "Paks");
        Directory.CreateDirectory(paks);
        File.WriteAllBytes(Path.Combine(paks, "container.pak"), [1]);
        var manifest = Path.Combine(steamApps, "appmanifest_1623730.acf");
        File.WriteAllText(
            manifest,
            "\"AppState\" { \"appid\" \"1623730\" \"installdir\" \"Palworld\" \"buildid\" \"99999999\" }");
        var arguments = new[]
        {
            "doctor",
            "--mappings", mapping,
            "--steam-root", root,
            "--steam-app-id", "1623730",
            "--build", Fixture.Build,
        };
        Assert.Equal(
            ExitCodes.MountSerialization,
            CommandRunner.Run(arguments, new Dictionary<string, string?>()));
        File.WriteAllText(
            manifest,
            "\"AppState\" { \"appid\" \"1623730\" \"installdir\" \"Palworld\" \"buildid\" \"24181527\" }");
        Assert.Equal(
            ExitCodes.Success,
            CommandRunner.Run(arguments, new Dictionary<string, string?>()));
    }
}
