using PalMapPack;
using CUE4Parse.UE4.Assets.Exports.Texture;
using System.Runtime.InteropServices;

namespace PalMapPack.Tests;

public sealed class LocalGameContractTests
{
    [Fact]
    public void DecodedMapRejectsValidLengthPfBgraInsteadOfColorSwappingIt()
    {
        var bytes = new byte[] { 1, 2, 3, 255 };

        var failure = Assert.Throws<MapPackFailure>(() =>
            Cue4ParseMapAssetReader.ValidateDecodedRgba(
                1,
                1,
                bytes,
                EPixelFormat.PF_B8G8R8A8));
        Assert.Equal(ExitCodes.MountSerialization, failure.ExitCode);

        var accepted = Cue4ParseMapAssetReader.ValidateDecodedRgba(
            1,
            1,
            bytes,
            EPixelFormat.PF_R8G8B8A8);
        Assert.Equal(bytes, accepted.Rgba);
    }

    [Fact]
    public void SteamLocatorFindsBuildAndCompleteContainerSetWithoutHardcodedInstallPath()
    {
        var root = Fixture.TempDirectory();
        var steamApps = Path.Combine(root, "steamapps");
        var install = Path.Combine(steamApps, "common", "Palworld");
        var paks = Path.Combine(install, "Pal", "Content", "Paks");
        Directory.CreateDirectory(paks);
        File.WriteAllText(Path.Combine(steamApps, "appmanifest_1623730.acf"),
            "\"AppState\" { \"appid\" \"1623730\" \"installdir\" \"Palworld\" \"buildid\" \"24181527\" }");
        File.WriteAllBytes(Path.Combine(paks, "a.pak"), [1]);
        File.WriteAllBytes(Path.Combine(paks, "b.utoc"), [2]);
        File.WriteAllBytes(Path.Combine(paks, "b.ucas"), [3]);

        var located = SteamInstallLocator.Locate("1623730", new[] { root });
        Assert.Equal(Fixture.Build, located.BuildId);
        var containers = AssetPaths.DiscoverContainers(located.InstallPath);
        Assert.Equal(new[] { "Pal/Content/Paks/a.pak", "Pal/Content/Paks/b.ucas", "Pal/Content/Paks/b.utoc" },
            containers.Select(container => container.RelativeName));
    }

    [Fact]
    public void ContainerSnapshotDetectsMutationAcrossExtraction()
    {
        var root = Fixture.TempDirectory();
        var path = Path.Combine(root, "a.pak");
        File.WriteAllBytes(path, [1, 2, 3]);
        var before = SourceContainerSnapshot.Capture(root, path);
        File.WriteAllBytes(path, [1, 2, 4]);
        var after = SourceContainerSnapshot.Capture(root, path);
        Assert.Throws<MapPackFailure>(() => SourceContainerSnapshot.EnsureUnchanged(new[] { before }, new[] { after }));
    }

    [Fact]
    public void ContainerSnapshotDetectsSameContentIdentityReplacement()
    {
        var root = Fixture.TempDirectory();
        var path = Path.Combine(root, "a.pak");
        var replacement = Path.Combine(root, "replacement.pak");
        var timestamp = new DateTime(2026, 1, 2, 3, 4, 5, DateTimeKind.Utc);
        File.WriteAllBytes(path, [1, 2, 3]);
        File.SetLastWriteTimeUtc(path, timestamp);
        var before = SourceContainerSnapshot.Capture(root, path);

        File.WriteAllBytes(replacement, [1, 2, 3]);
        File.SetLastWriteTimeUtc(replacement, timestamp);
        File.Move(replacement, path, overwrite: true);
        var after = SourceContainerSnapshot.Capture(root, path);

        Assert.Equal(before.Sha256, after.Sha256);
        Assert.Equal(before.LastWriteUtcTicks, after.LastWriteUtcTicks);
        Assert.Throws<MapPackFailure>(() =>
            SourceContainerSnapshot.EnsureUnchanged([before], [after]));
    }

    [Fact]
    public void MappingHashRejectsHardLinks()
    {
        var root = Fixture.TempDirectory();
        var original = Path.Combine(root, "mapping.usmap");
        var hardLink = Path.Combine(root, "linked.usmap");
        File.WriteAllBytes(original, [1, 2, 3]);
        Assert.True(CreateHardLink(hardLink, original, IntPtr.Zero));

        var failure = Assert.Throws<MapPackFailure>(() => Hashing.Sha256File(hardLink));

        Assert.Equal(ExitCodes.PackIntegrity, failure.ExitCode);
    }

    [Fact]
    public void AssetContractLoadRejectsAReparseFile()
    {
        var root = Fixture.TempDirectory();
        var outside = Path.Combine(Fixture.TempDirectory(), "contract.json");
        File.WriteAllText(outside, "{}");
        var link = Path.Combine(root, "contract.json");
        try
        {
            File.CreateSymbolicLink(link, outside);
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException)
        {
            return;
        }

        var failure = Assert.Throws<MapPackFailure>(() => AssetContract.Load(link));

        Assert.Equal(ExitCodes.AssetContractMismatch, failure.ExitCode);
    }

    [Fact]
    public void AssetContractLoadRejectsSyntacticallyValidButIncompleteJson()
    {
        var root = Fixture.TempDirectory();
        var path = Path.Combine(root, "incomplete-contract.json");
        File.WriteAllText(path, "{\"game_build_id\":\"24181527\"}");

        var failure = Assert.Throws<MapPackFailure>(() => AssetContract.Load(path));
        Assert.Equal(ExitCodes.AssetContractMismatch, failure.ExitCode);
    }

    [Fact]
    public void AssetContractParseAppliesTheSameSemanticValidationAsFileLoad()
    {
        var failure = Assert.Throws<MapPackFailure>(() =>
            AssetContract.Parse("{\"game_build_id\":\"24181527\"}"u8));

        Assert.Equal(ExitCodes.AssetContractMismatch, failure.ExitCode);
    }

    [Fact]
    public void AssetContractParseRejectsDuplicateSentinelIdentityAndProperties()
    {
        var contract = Fixture.ApprovedContract();
        var duplicateSentinel = contract with
        {
            Sentinels = contract.Sentinels.Concat([
                contract.Sentinels[0] with
                {
                    PackagePath = contract.Sentinels[0].PackagePath.ToUpperInvariant(),
                    ExportClass = contract.Sentinels[0].ExportClass.ToLowerInvariant(),
                },
            ]).ToArray(),
        };
        var duplicateProperty = contract with
        {
            Sentinels = contract.Sentinels.Select((sentinel, index) => index == 0
                ? sentinel with
                {
                    RequiredProperties = sentinel.RequiredProperties
                        .Concat([sentinel.RequiredProperties[0]])
                        .ToArray(),
                }
                : sentinel).ToArray(),
        };

        Assert.Throws<MapPackFailure>(() =>
            AssetContract.Parse(ManifestWriter.SerializeBytes(duplicateSentinel)));
        Assert.Throws<MapPackFailure>(() =>
            AssetContract.Parse(ManifestWriter.SerializeBytes(duplicateProperty)));
    }

    [DllImport("kernel32.dll", EntryPoint = "CreateHardLinkW", SetLastError = true, CharSet = CharSet.Unicode)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CreateHardLink(
        string fileName,
        string existingFileName,
        IntPtr securityAttributes);

    [Fact]
    public void ContainerDiscoveryRejectsAReparseDirectoryThatEscapesTheInstallRoot()
    {
        var install = Fixture.TempDirectory();
        var paks = Path.Combine(install, "Pal", "Content", "Paks");
        var outside = Fixture.TempDirectory();
        Directory.CreateDirectory(paks);
        File.WriteAllBytes(Path.Combine(outside, "escaped.pak"), [1, 2, 3]);
        var link = Path.Combine(paks, "linked");
        try
        {
            Directory.CreateSymbolicLink(link, outside);
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException)
        {
            return;
        }

        var failure = Assert.Throws<MapPackFailure>(() =>
            AssetPaths.DiscoverContainers(install));

        Assert.Equal(ExitCodes.PackIntegrity, failure.ExitCode);
    }

    [Fact]
    public void AssetContractProbeRequiresExactReviewedSentinelShapes()
    {
        var contract = Fixture.ApprovedContract();
        var inventory = new AssetInventory(new[]
        {
            new AssetInventoryEntry("map", "Texture2D", new[] { "SizeX", "SizeY" }),
        });
        AssetCatalogProbe.EnsureContractAccepted(contract, Fixture.Build, inventory);
        var changed = new AssetInventory(new[]
        {
            new AssetInventoryEntry("map", "Texture2D", new[] { "SizeX" }),
        });
        Assert.Throws<MapPackFailure>(() =>
            AssetCatalogProbe.EnsureContractAccepted(contract, Fixture.Build, changed));
    }

    [Fact]
    public void SteamLocatorDiscoversSecondaryLibrariesFromLibraryFolders()
    {
        var steamRoot = Fixture.TempDirectory();
        var secondary = Fixture.TempDirectory();
        Directory.CreateDirectory(Path.Combine(steamRoot, "steamapps"));
        File.WriteAllText(
            Path.Combine(steamRoot, "steamapps", "libraryfolders.vdf"),
            $"\"libraryfolders\" {{ \"1\" {{ \"path\" \"{secondary.Replace("\\", "\\\\", StringComparison.Ordinal)}\" }} }}");
        var steamApps = Path.Combine(secondary, "steamapps");
        var install = Path.Combine(steamApps, "common", "Palworld");
        Directory.CreateDirectory(install);
        File.WriteAllText(
            Path.Combine(steamApps, "appmanifest_1623730.acf"),
            "\"AppState\" { \"appid\" \"1623730\" \"installdir\" \"Palworld\" \"buildid\" \"24181527\" }");

        var libraries = SteamInstallLocator.DiscoverLibraryRoots(steamRoot);
        var located = SteamInstallLocator.Locate("1623730", libraries);
        Assert.Equal(Path.GetFullPath(install), located.InstallPath);
    }

    [Fact]
    [Trait("Category", "LocalGame")]
    public void Build24181527ProbeDecodesEveryReviewedAuthoritativeMapRegion()
    {
        var mappings = Environment.GetEnvironmentVariable("PAL_USMAP_PATH");
        if (string.IsNullOrWhiteSpace(mappings) || !File.Exists(mappings))
        {
            return;
        }
        var install = SteamInstallLocator.LocateDefault("1623730");
        if (install.BuildId != Fixture.Build)
        {
            return;
        }
        var contractPath = Path.GetFullPath(Path.Combine(
            AppContext.BaseDirectory,
            "..", "..", "..", "..", "..",
            "contracts", "24181527.asset-contract.json"));
        var contract = AssetContract.Load(contractPath);

        var probe = new Cue4ParseMapAssetReader().Probe(
            new AssetProbeRequest(install.InstallPath, install.BuildId, mappings, contract));

        Assert.True(contract.Reviewed);
        Assert.Equal(2, probe.Regions.All.Count);
        var main = Assert.Single(probe.Regions.All, region => region.MapId == "MainMap");
        Assert.Equal(
            "/Game/Pal/Texture/UI/Map/T_WorldMap.T_WorldMap",
            main.SourceTexturePath);
        Assert.Equal((-1_099_400.0, -724_400.0, 349_400.0, 724_400.0),
            (main.WorldMinX, main.WorldMinY, main.WorldMaxX, main.WorldMaxY));
        Assert.Equal(0, main.Priority);
        Assert.Equal((8192, 8192), (main.Map.Width, main.Map.Height));
        var tree = Assert.Single(probe.Regions.All, region => region.MapId == "Tree");
        Assert.Equal(
            "/Game/Pal/Texture/UI/Map/T_TreeMap.T_TreeMap",
            tree.SourceTexturePath);
        Assert.Equal((347_351.5, -818_197.0, 689_148.5, -476_400.0),
            (tree.WorldMinX, tree.WorldMinY, tree.WorldMaxX, tree.WorldMaxY));
        Assert.Equal(1, tree.Priority);
        Assert.Equal((8192, 8192), (tree.Map.Width, tree.Map.Height));
        Assert.NotEqual(
            Hashing.MapAssetSha256(main.Map),
            Hashing.MapAssetSha256(tree.Map));
        var poi = Assert.IsType<PoiProbeSummary>(probe.PoiSummary);
        Assert.Equal(31_159, poi.WorldExportCount);
        Assert.Equal(199, poi.FastTravelTextRowCount);
        Assert.Equal(152, poi.FastTravelCount);
        Assert.Equal(157, poi.DungeonCount);
        Assert.Equal(159, poi.BossRawCount);
        Assert.Equal(126, poi.BossSemanticCount);
        Assert.Equal(90, poi.PalBossCount);
        Assert.Equal(33, poi.HumanBossCount);
        Assert.Equal(3, poi.BossRegionCount);
        Assert.Equal(33, poi.BossDuplicateCount);

        var poisonedSentinels = contract.Sentinels
            .Select((sentinel, index) => index == 0
                ? sentinel with
                {
                    RequiredProperties = sentinel.RequiredProperties
                        .Concat(["InventedByContract"])
                        .ToArray(),
                }
                : sentinel)
            .ToArray();
        var independentlyInventoried = new Cue4ParseMapAssetReader().Probe(
            new AssetProbeRequest(
                install.InstallPath,
                install.BuildId,
                mappings,
                contract with { Sentinels = poisonedSentinels }));
        Assert.DoesNotContain(
            "InventedByContract",
            independentlyInventoried.Inventory.Entries
                .Single(entry => entry.PackagePath == poisonedSentinels[0].PackagePath)
                .Properties);
    }
}
