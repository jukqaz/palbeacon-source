using PalMapPack;

namespace PalMapPack.Tests;

public sealed class CommandPipelineTests
{
    [Fact]
    public void CandidateContractCanPrepareASealedCommitmentWithoutResidualsOrReview()
    {
        var fixture = CommandFixture.Create(reviewed: false);
        var services = new CommandServices(
            _ => throw new InvalidOperationException("commitment preparation must not probe assets"),
            _ => throw new InvalidOperationException("commitment preparation must not decode assets"));

        Assert.Equal(
            ExitCodes.Success,
            CommandRunner.Run(
                fixture.HoldoutArguments(),
                new Dictionary<string, string?>(),
                services));

        using var document = System.Text.Json.JsonDocument.Parse(
            File.ReadAllBytes(fixture.CommitmentOutputPath));
        var root = document.RootElement;
        Assert.Equal(1u, root.GetProperty("schema_version").GetUInt32());
        Assert.Equal(Fixture.Build, root.GetProperty("game_build_id").GetString());
        Assert.Equal(5u, root.GetProperty("sealed_holdout_count").GetUInt32());
        Assert.Matches(
            "^[0-9a-f]{64}$",
            root.GetProperty("sealed_holdout_sha256").GetString());
        Assert.Equal(1u, root.GetProperty("center_count").GetUInt32());
        Assert.Equal(1u, root.GetProperty("north_west_count").GetUInt32());
        Assert.Equal(1u, root.GetProperty("north_east_count").GetUInt32());
        Assert.Equal(1u, root.GetProperty("south_west_count").GetUInt32());
        Assert.Equal(1u, root.GetProperty("south_east_count").GetUInt32());
        Assert.DoesNotContain("error", root.ToString(), StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain("world_", root.ToString(), StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain("map_", root.ToString(), StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void HoldoutCommitmentOutputCannotOverwriteAnyProtectedInput()
    {
        var fixture = CommandFixture.Create(reviewed: false);
        var protectedPaths = new[]
        {
            fixture.HoldoutsPath,
            fixture.ContractPath,
            fixture.MappingPath,
        };
        var before = protectedPaths.ToDictionary(
            path => path,
            Hashing.Sha256File,
            StringComparer.OrdinalIgnoreCase);

        foreach (var protectedPath in protectedPaths)
        {
            var arguments = fixture.HoldoutArguments();
            arguments[^1] = protectedPath;
            Assert.Equal(
                ExitCodes.CalibrationRejected,
                CommandRunner.Run(
                    arguments,
                    new Dictionary<string, string?>(),
                    new CommandServices(
                        _ => throw new InvalidOperationException(),
                        _ => throw new InvalidOperationException())));
        }
        Assert.All(
            before,
            pair => Assert.Equal(pair.Value, Hashing.Sha256File(pair.Key)));
    }

    [Fact]
    public void CandidateContractCanBeProbedButCannotBeStaged()
    {
        var fixture = CommandFixture.Create(reviewed: false);
        var probeCalls = 0;
        var services = new CommandServices(
            request =>
            {
                probeCalls++;
                Assert.Equal(Fixture.Build, request.GameBuildId);
                Assert.Equal(fixture.InstallPath, request.InstallPath);
                return new AssetProbeResult(Fixture.Assets().Regions, Fixture.Inventory());
            },
            _ => new FixtureAssetReader(Fixture.Assets()));

        Assert.Equal(
            ExitCodes.Success,
            CommandRunner.Run(fixture.Arguments("probe-assets"), new Dictionary<string, string?>(), services));
        Assert.Equal(1, probeCalls);
        var reportPath = Path.Combine(
            fixture.DatasetRoot,
            $".{Fixture.Build}.pipeline",
            "probe.json");
        Assert.True(File.Exists(reportPath));
        using (var report = System.Text.Json.JsonDocument.Parse(File.ReadAllBytes(reportPath)))
        {
            var root = report.RootElement;
            Assert.Equal(2u, root.GetProperty("schema_version").GetUInt32());
            Assert.Equal("CANDIDATE_PROBED_NOT_ACCEPTED", root.GetProperty("status").GetString());
            Assert.Equal(Fixture.Build, root.GetProperty("game_build_id").GetString());
            Assert.Equal(2, root.GetProperty("map_regions").GetArrayLength());
            Assert.NotEmpty(root.GetProperty("asset_inventory").EnumerateArray());
            Assert.False(root.ToString().Contains(fixture.InstallPath, StringComparison.OrdinalIgnoreCase));
        }

        Assert.Equal(
            ExitCodes.AssetContractMismatch,
            CommandRunner.Run(fixture.Arguments("stage"), new Dictionary<string, string?>(), services));
        Assert.Equal(1, probeCalls);
    }

    [Fact]
    public void ProbeRejectsAContractBoundToDifferentPreviewBytesBeforePublishingThePreview()
    {
        var fixture = CommandFixture.Create(reviewed: false);
        var contract = AssetContract.Load(fixture.ContractPath);
        var regions = contract.MapRegions.Select(region =>
            region.MapId == "MainMap" && region.RegionId == "FirstRegion"
                ? region with
                {
                    CalibrationPreview = region.CalibrationPreview! with
                    {
                        Sha256 = Hashing.Sha256Hex("different-preview"u8),
                    },
                }
                : region).ToArray();
        File.WriteAllBytes(
            fixture.ContractPath,
            ManifestWriter.SerializeBytes(contract with { MapRegions = regions }));
        var services = new CommandServices(
            _ => new AssetProbeResult(Fixture.Assets().Regions, Fixture.Inventory()),
            _ => new FixtureAssetReader(Fixture.Assets()));

        Assert.Equal(
            ExitCodes.AssetContractMismatch,
            CommandRunner.Run(
                fixture.Arguments("probe-assets"),
                new Dictionary<string, string?>(),
                services));
        Assert.False(File.Exists(Path.Combine(
            fixture.DatasetRoot,
            $".{Fixture.Build}.pipeline",
            "calibration-preview-mainmap.bmp")));
    }

    [Fact]
    public void CandidateMappingMismatchFailsBeforeTheLocalProviderIsOpened()
    {
        var fixture = CommandFixture.Create(reviewed: false);
        File.AppendAllText(fixture.MappingPath, "tampered");
        var probeCalls = 0;
        var services = new CommandServices(
            _ =>
            {
                probeCalls++;
                return new AssetProbeResult(Fixture.Assets().Regions, Fixture.Inventory());
            },
            _ => new FixtureAssetReader(Fixture.Assets()));

        Assert.Equal(
            ExitCodes.AssetContractMismatch,
            CommandRunner.Run(
                fixture.Arguments("probe-assets"),
                new Dictionary<string, string?>(),
                services));
        Assert.Equal(0, probeCalls);
    }

    [Fact]
    public void ReviewedExactBuildRunsStageCalibrateValidateAndPublish()
    {
        var fixture = CommandFixture.Create(reviewed: true);
        IReadOnlyList<string>? factoryPaths = null;
        IReadOnlyList<string>? readPaths = null;
        var services = new CommandServices(
            _ => new AssetProbeResult(Fixture.Assets().Regions, Fixture.Inventory()),
            paths =>
            {
                factoryPaths = paths.ToArray();
                return new CapturingAssetReader(
                    request => readPaths = request.ContainerPaths.ToArray());
            });
        var environment = new Dictionary<string, string?>();

        foreach (var command in new[]
                 {
                     "accept-contract",
                     "stage",
                     "calibrate",
                     "validate",
                     "publish",
                 })
        {
            var result = CommandRunner.Run(fixture.Arguments(command), environment, services);
            Assert.True(
                result == ExitCodes.Success,
                $"{command} returned {result} instead of {ExitCodes.Success}");
        }
        var expectedContainer = Path.Combine(
            fixture.InstallPath,
            "Pal",
            "Content",
            "Paks",
            "container.pak");
        Assert.Equal([Path.GetFullPath(expectedContainer)], factoryPaths);
        Assert.Equal([Path.GetFullPath(expectedContainer)], readPaths);
        Assert.All(readPaths!, path => Assert.True(Path.IsPathFullyQualified(path)));

        var target = AtomicPublisher.ResolveActivePath(fixture.DatasetRoot, Fixture.Build);
        Assert.True(File.Exists(Path.Combine(target, "manifest.json")));
        Assert.True(MapPackValidator.IsValid(
            target,
            Fixture.Build,
            Hashing.Sha256File(fixture.ContractPath),
            Hashing.Sha256File(fixture.MappingPath)));
    }

    [Fact]
    public void ValidatorRejectsAnUnindexedReparseDirectoryInTheTileTree()
    {
        var fixture = CommandFixture.Create(reviewed: true);
        var services = new CommandServices(
            _ => new AssetProbeResult(Fixture.Assets().Regions, Fixture.Inventory()),
            _ => new FixtureAssetReader(Fixture.Assets()));
        foreach (var command in new[]
                 {
                     "probe-assets",
                     "accept-contract",
                     "stage",
                     "calibrate",
                     "validate",
                 })
        {
            Assert.Equal(
                ExitCodes.Success,
                CommandRunner.Run(
                    fixture.Arguments(command),
                    new Dictionary<string, string?>(),
                    services));
        }

        var statePath = Path.Combine(
            fixture.DatasetRoot,
            $".{Fixture.Build}.pipeline",
            "validated.json");
        using var state = System.Text.Json.JsonDocument.Parse(File.ReadAllBytes(statePath));
        var staging = state.RootElement.GetProperty("staging_path").GetString()!;
        var outside = Fixture.TempDirectory();
        try
        {
            Directory.CreateSymbolicLink(
                Path.Combine(
                    staging,
                    "regions",
                    "mainmap",
                    "firstregion",
                    "tiles",
                    "escape"),
                outside);
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException)
        {
            return;
        }

        Assert.False(MapPackValidator.IsValid(
            staging,
            Fixture.Build,
            Hashing.Sha256File(fixture.ContractPath),
            Hashing.Sha256File(fixture.MappingPath)));
    }

    private sealed class CapturingAssetReader(Action<AssetReadRequest> capture) : IMapAssetReader
    {
        public MapAssetSet Read(AssetReadRequest request)
        {
            capture(request);
            return Fixture.Assets();
        }
    }

    private sealed record CommandFixture(
        string Root,
        string SteamRoot,
        string InstallPath,
        string MappingPath,
        string ContractPath,
        string DatasetRoot,
        string LandmarksPath,
        string HoldoutsPath,
        string CommitmentOutputPath)
    {
        public static CommandFixture Create(bool reviewed)
        {
            var root = Fixture.TempDirectory();
            var mappingPath = Path.Combine(root, "fixture.usmap");
            File.WriteAllBytes(mappingPath, "mapping"u8.ToArray());

            var contract = Fixture.ApprovedContract();
            if (!reviewed)
            {
                contract = contract with
                {
                    Reviewed = false,
                    ReviewId = string.Empty,
                    ApprovedMappingSha256 = null,
                    ApprovedSealedHoldoutSha256 = null,
                    MappingCandidate = new MappingCandidateProvenance(
                        "fixture/repository",
                        "01234567",
                        "2026-07-17",
                        new FileInfo(mappingPath).Length,
                        Hashing.Sha256File(mappingPath),
                        "candidate_unverified"),
                };
            }
            var contractPath = Path.Combine(root, "contract.json");
            File.WriteAllBytes(contractPath, ManifestWriter.SerializeBytes(contract));

            var steamApps = Path.Combine(root, "steamapps");
            var installPath = Path.Combine(steamApps, "common", "Palworld");
            var paks = Path.Combine(installPath, "Pal", "Content", "Paks");
            Directory.CreateDirectory(paks);
            File.WriteAllBytes(Path.Combine(paks, "container.pak"), [1, 2, 3]);
            File.WriteAllText(
                Path.Combine(steamApps, "appmanifest_1623730.acf"),
                "\"AppState\" { \"appid\" \"1623730\" \"installdir\" \"Palworld\" \"buildid\" \"24181527\" }");

            var datasetRoot = Path.Combine(root, "datasets");
            Directory.CreateDirectory(datasetRoot);
            var landmarksPath = Path.Combine(root, "landmarks.json");
            var holdoutsPath = Path.Combine(root, "sealed-holdouts.json");
            var commitmentOutputPath = Path.Combine(root, "holdout-commitment.json");
            File.WriteAllBytes(
                landmarksPath,
                ManifestWriter.SerializeBytes(Fixture.ExactLandmarks().Select(landmark => new
                {
                    landmark.Id,
                    landmark.GameBuildId,
                    EvidenceSet = landmark.EvidenceSet == LandmarkEvidenceSet.Reference
                        ? "reference"
                        : "sealed_holdout",
                    landmark.WorldX,
                    landmark.WorldY,
                    landmark.MapX,
                    landmark.MapY,
                })));
            File.WriteAllBytes(
                holdoutsPath,
                ManifestWriter.SerializeBytes(Fixture.ExactLandmarks()
                    .Where(landmark =>
                        landmark.EvidenceSet == LandmarkEvidenceSet.SealedHoldout)
                    .Select(landmark => new
                    {
                        landmark.Id,
                        landmark.GameBuildId,
                        EvidenceSet = "sealed_holdout",
                        landmark.WorldX,
                        landmark.WorldY,
                        landmark.MapX,
                        landmark.MapY,
                    })));
            return new CommandFixture(
                root,
                root,
                Path.GetFullPath(installPath),
                mappingPath,
                contractPath,
                datasetRoot,
                landmarksPath,
                holdoutsPath,
                commitmentOutputPath);
        }

        public string[] Arguments(string command) =>
        [
            command,
            "--steam-app-id", "1623730",
            "--steam-root", SteamRoot,
            "--build", Fixture.Build,
            "--mappings", MappingPath,
            "--contract", ContractPath,
            "--dataset-root", DatasetRoot,
            "--landmarks", LandmarksPath,
        ];

        public string[] HoldoutArguments() =>
        [
            "prepare-holdout-commitment",
            "--steam-app-id", "1623730",
            "--steam-root", SteamRoot,
            "--build", Fixture.Build,
            "--mappings", MappingPath,
            "--contract", ContractPath,
            "--landmarks", HoldoutsPath,
            "--commitment-output", CommitmentOutputPath,
        ];
    }
}
