using PalMapPack;

namespace PalMapPack.Tests;

public sealed class ManifestTests
{
    [Fact]
    public void ManifestContainsEveryCompatibilityAndProvenanceHash()
    {
        var manifest = ManifestWriter.Create(
            Fixture.Build,
            new[] { Fixture.Container() },
            Hashing.Sha256Hex("contract"u8),
            Hashing.Sha256Hex("mapping"u8),
            Fixture.ManifestRegions(),
            Hashing.Sha256Hex("pois"u8),
            "fixture",
            "0123456789abcdef",
            DateTimeOffset.UnixEpoch);

        Assert.Equal(Fixture.Build, manifest.GameBuildId);
        Assert.All(new[]
        {
            manifest.SourceInputSetSha256, manifest.SourceContractSha256, manifest.MappingSha256,
            manifest.PoisSha256,
        }, hash => Assert.Matches("^[0-9a-f]{64}$", hash));
        Assert.NotEmpty(manifest.SourceContainers);
        Assert.All(manifest.SourceContainers, source =>
        {
            Assert.False(Path.IsPathRooted(source.RelativeName));
            Assert.True(source.SizeBytes > 0);
            Assert.Matches("^[0-9a-f]{64}$", source.Sha256);
        });
        Assert.Equal("1.2.2.202607", manifest.Cue4ParseVersion);
        Assert.Equal(512u, manifest.TileCoreSizePx);
        Assert.Equal(2u, manifest.TileGutterPx);
        Assert.Equal(2u, manifest.CoordinateTransformVersion);
        Assert.Equal(2u, manifest.PoiSchemaVersion);
        Assert.Equal(2, manifest.MapRegions.Count);
        var main = Assert.Single(manifest.MapRegions, region => region.MapId == "MainMap");
        Assert.Equal("/Game/Pal/Texture/UI/Map/T_WorldMap.T_WorldMap", main.SourceTexturePath);
        Assert.Equal((-1_099_400.0, -724_400.0, 349_400.0, 724_400.0),
            (main.WorldMinX, main.WorldMinY, main.WorldMaxX, main.WorldMaxY));
        Assert.Equal((1.0, 1.0, 0.0, 0.0, 0),
            (main.BlockSizeX, main.BlockSizeY, main.GridPositionX, main.GridPositionY, main.Priority));
        Assert.All(manifest.MapRegions, region =>
        {
            Assert.InRange(region.MapWidthPx, 1, 8192);
            Assert.InRange(region.MapHeightPx, 1, 8192);
            Assert.All(new[]
            {
                region.MapAssetSha256,
                region.TransformSha256,
                region.TileSetSha256,
                region.TileIndexSha256,
            }, hash => Assert.Matches("^[0-9a-f]{64}$", hash));
        });
    }

    [Fact]
    public void SourceInputHashIsOrderIndependentAndBindsEveryContainerField()
    {
        var first = Fixture.Container("Pal/Z.pak");
        var second = new SourceContainerSnapshot(
            "Pal/A.utoc",
            11,
            first.LastWriteUtcTicks,
            Hashing.Sha256Hex("other"u8));
        var left = Hashing.SourceInputSetSha256(new[] { first, second });
        var right = Hashing.SourceInputSetSha256(new[] { second, first });
        Assert.Equal(left, right);
        Assert.NotEqual(left, Hashing.SourceInputSetSha256(new[] { first }));
    }

    [Fact]
    public void ManifestSerializationIsCanonicalAndDeterministic()
    {
        var manifest = ManifestWriter.Create(
            Fixture.Build,
            new[] { Fixture.Container() },
            new string('1', 64), new string('2', 64), Fixture.ManifestRegions(),
            new string('5', 64), "1.0.0", "abc", DateTimeOffset.UnixEpoch);
        Assert.Equal(ManifestWriter.Serialize(manifest), ManifestWriter.Serialize(manifest));
        Assert.EndsWith("\n", ManifestWriter.Serialize(manifest), StringComparison.Ordinal);
    }

    [Fact]
    public void DeterministicDoubleBuildProducesIdenticalCanonicalHashesAndManifestLast()
    {
        var assets = Fixture.Assets();
        var contract = Fixture.ApprovedContract();
        var first = MapPackBuilder.BuildStaging(new MapPackBuildRequest(
            Fixture.TempDirectory(),
            Fixture.Build,
            "fixture.usmap",
            "mapping"u8.ToArray(),
            "contract"u8.ToArray(),
            contract,
            new[] { Fixture.Container() },
            Fixture.ContainerPaths(),
            Fixture.ExactLandmarks(),
            new FixtureAssetReader(assets),
            DateTimeOffset.UnixEpoch,
            "fixture",
            "abc",
            Fixture.Inventory()));
        var second = MapPackBuilder.BuildStaging(new MapPackBuildRequest(
            Fixture.TempDirectory(),
            Fixture.Build,
            "fixture.usmap",
            "mapping"u8.ToArray(),
            "contract"u8.ToArray(),
            contract,
            new[] { Fixture.Container() },
            Fixture.ContainerPaths(),
            Fixture.ExactLandmarks(),
            new FixtureAssetReader(assets),
            DateTimeOffset.UnixEpoch,
            "fixture",
            "abc",
            Fixture.Inventory()));

        Assert.Equal(ManifestWriter.Serialize(first.Manifest), ManifestWriter.Serialize(second.Manifest));
        Assert.True(File.Exists(Path.Combine(first.StagingPath, "manifest.json")));
        Assert.Equal("manifest.json", first.WriteOrder[^1]);
        Assert.Equal(2u, first.Manifest.SchemaVersion);
        var poiDocument = System.Text.Json.JsonSerializer.Deserialize<PoiDocument>(
            File.ReadAllBytes(Path.Combine(first.StagingPath, "pois.json")),
            new System.Text.Json.JsonSerializerOptions
            {
                PropertyNamingPolicy = System.Text.Json.JsonNamingPolicy.SnakeCaseLower,
            });
        Assert.NotNull(poiDocument);
        Assert.Equal(
            "Anubis",
            Assert.Single(poiDocument.Pois, poi => poi.Kind == "boss").EntityId);
        Assert.Single(poiDocument.Pois, poi => poi.Kind == "wanted");
        var mainRegion = Assert.Single(
            first.Manifest.MapRegions,
            region => region.MapId == "MainMap");
        var treeRegion = Assert.Single(
            first.Manifest.MapRegions,
            region => region.MapId == "Tree");
        Assert.Equal(
            "regions/mainmap/firstregion/tile-index.json",
            mainRegion.TileIndexRelativePath);
        Assert.Equal(
            "regions/tree/dummyregion/tile-index.json",
            treeRegion.TileIndexRelativePath);
        Assert.Equal(
            "regions/tree/dummyregion/transform.json",
            treeRegion.TransformRelativePath);
        var mainTransform = System.Text.Json.JsonSerializer.Deserialize<TransformDocument>(
            File.ReadAllBytes(Path.Combine(
                first.StagingPath,
                mainRegion.TransformRelativePath.Replace('/', Path.DirectorySeparatorChar))),
            new System.Text.Json.JsonSerializerOptions
            {
                PropertyNamingPolicy = System.Text.Json.JsonNamingPolicy.SnakeCaseLower,
            });
        Assert.NotNull(mainTransform);
        Assert.Equal("authoritative_world_bounds_validated", mainTransform.TransformKind);
        Assert.NotEqual(mainRegion.MapAssetSha256, treeRegion.MapAssetSha256);
        Assert.NotEqual(mainRegion.TileSetSha256, treeRegion.TileSetSha256);
        Assert.True(File.Exists(Path.Combine(
            first.StagingPath,
            treeRegion.TileIndexRelativePath.Replace('/', Path.DirectorySeparatorChar))));
        Assert.All(
            first.Manifest.MapRegions,
            region =>
            {
                Assert.Equal(2, region.WorldToMapMatrix.Length);
                Assert.All(region.WorldToMapMatrix, row => Assert.Equal(3, row.Length));
                Assert.Matches("^[0-9a-f]{64}$", region.TransformSha256);
                Assert.Matches("^[0-9a-f]{64}$", region.TileIndexSha256);
                Assert.Matches("^[0-9a-f]{64}$", region.TileSetSha256);
            });
        Assert.True(MapPackValidator.IsValid(
            first.StagingPath,
            Fixture.Build,
            Hashing.Sha256Hex("contract"u8),
            Hashing.Sha256Hex("mapping"u8)));
    }

    [Fact]
    public void BuilderChecksExactBuildAndContainerInputsBeforeAndAfterReading()
    {
        var guard = new CountingInputGuard();
        MapPackBuilder.BuildStaging(new MapPackBuildRequest(
            Fixture.TempDirectory(),
            Fixture.Build,
            "fixture.usmap",
            "mapping"u8.ToArray(),
            "contract"u8.ToArray(),
            Fixture.ApprovedContract(),
            new[] { Fixture.Container() },
            Fixture.ContainerPaths(),
            Fixture.ExactLandmarks(),
            new FixtureAssetReader(Fixture.Assets()),
            DateTimeOffset.UnixEpoch,
            "fixture",
            "abc",
            Fixture.Inventory(),
            guard));
        Assert.Equal(2, guard.Checks);
    }

    [Fact]
    public void ValidatorRejectsSchemaV1AndLegacyGlobalArtifactFallbacks()
    {
        var build = MapPackBuilder.BuildStaging(new MapPackBuildRequest(
            Fixture.TempDirectory(),
            Fixture.Build,
            "fixture.usmap",
            "mapping"u8.ToArray(),
            "contract"u8.ToArray(),
            Fixture.ApprovedContract(),
            new[] { Fixture.Container() },
            Fixture.ContainerPaths(),
            Fixture.ExactLandmarks(),
            new FixtureAssetReader(Fixture.Assets()),
            DateTimeOffset.UnixEpoch,
            "fixture",
            "abc",
            Fixture.Inventory()));
        var manifestPath = Path.Combine(build.StagingPath, "manifest.json");
        var manifestBytes = File.ReadAllBytes(manifestPath);
        var schemaV1 = System.Text.Encoding.UTF8.GetString(manifestBytes)
            .Replace("\"schema_version\": 2", "\"schema_version\": 1", StringComparison.Ordinal);
        File.WriteAllText(manifestPath, schemaV1);

        Assert.False(MapPackValidator.IsValid(
            build.StagingPath,
            Fixture.Build,
            Hashing.Sha256Hex("contract"u8),
            Hashing.Sha256Hex("mapping"u8)));

        File.WriteAllBytes(manifestPath, manifestBytes);
        File.WriteAllText(Path.Combine(build.StagingPath, "transform.json"), "{}");
        Assert.False(MapPackValidator.IsValid(
            build.StagingPath,
            Fixture.Build,
            Hashing.Sha256Hex("contract"u8),
            Hashing.Sha256Hex("mapping"u8)));
    }

    [Fact]
    public void RustGoldenSchemaV2FixturePassesTheDotNetValidator()
    {
        var workspace = Path.GetFullPath(Path.Combine(
            AppContext.BaseDirectory,
            "..",
            "..",
            "..",
            "..",
            "..",
            "..",
            ".."));
        var fixture = Path.Combine(
            workspace,
            "tests",
            "fixtures",
            "map-pack-valid");

        Assert.True(MapPackValidator.IsValid(
            fixture,
            Fixture.Build,
            new string('4', 64),
            new string('5', 64)));
    }

    [Fact]
    public void BuilderRejectsMappingBytesThatDoNotMatchTheReviewedContract()
    {
        var failure = Assert.Throws<MapPackFailure>(() =>
            MapPackBuilder.BuildStaging(new MapPackBuildRequest(
                Fixture.TempDirectory(),
                Fixture.Build,
                "fixture.usmap",
                "different"u8.ToArray(),
                "contract"u8.ToArray(),
                Fixture.ApprovedContract(),
                new[] { Fixture.Container() },
                Fixture.ContainerPaths(),
                Fixture.ExactLandmarks(),
                new FixtureAssetReader(Fixture.Assets()),
                DateTimeOffset.UnixEpoch,
                "fixture",
                "abc",
                Fixture.Inventory())));
        Assert.Equal(ExitCodes.AssetContractMismatch, failure.ExitCode);
    }

    [Fact]
    public void BuilderRejectsTreeIdentityBackedByMainMapPixels()
    {
        var assets = Fixture.Assets();
        var aliased = new MapAssetSet(
            new MapRegionSet(assets.Regions.All.Select(region =>
                region.MapId == "Tree"
                    ? region with { Map = assets.MainMap.Map }
                    : region)),
            assets.PoiRows);

        var failure = Assert.Throws<MapPackFailure>(() =>
            MapPackBuilder.BuildStaging(new MapPackBuildRequest(
                Fixture.TempDirectory(),
                Fixture.Build,
                "fixture.usmap",
                "mapping"u8.ToArray(),
                "contract"u8.ToArray(),
                Fixture.ApprovedContract(),
                new[] { Fixture.Container() },
                Fixture.ContainerPaths(),
                Fixture.ExactLandmarks(),
                new FixtureAssetReader(aliased),
                DateTimeOffset.UnixEpoch,
                "fixture",
                "abc",
                Fixture.Inventory())));

        Assert.Equal(ExitCodes.PackIntegrity, failure.ExitCode);
    }

    [Fact]
    public void ValidatorRejectsAuthoritativeMetadataAndContentAliasing()
    {
        var build = MapPackBuilder.BuildStaging(new MapPackBuildRequest(
            Fixture.TempDirectory(),
            Fixture.Build,
            "fixture.usmap",
            "mapping"u8.ToArray(),
            "contract"u8.ToArray(),
            Fixture.ApprovedContract(),
            new[] { Fixture.Container() },
            Fixture.ContainerPaths(),
            Fixture.ExactLandmarks(),
            new FixtureAssetReader(Fixture.Assets()),
            DateTimeOffset.UnixEpoch,
            "fixture",
            "abc",
            Fixture.Inventory()));
        var main = Assert.Single(
            build.Manifest.MapRegions,
            region => region.MapId == "MainMap");
        var manifestPath = Path.Combine(build.StagingPath, "manifest.json");

        var mapAliased = build.Manifest with
        {
            MapRegions = build.Manifest.MapRegions.Select(region =>
                region.MapId == "Tree"
                    ? region with { MapAssetSha256 = main.MapAssetSha256 }
                    : region).ToArray(),
        };
        File.WriteAllBytes(manifestPath, ManifestWriter.SerializeBytes(mapAliased));
        Assert.False(MapPackValidator.IsValid(
            build.StagingPath,
            Fixture.Build,
            Hashing.Sha256Hex("contract"u8),
            Hashing.Sha256Hex("mapping"u8)));

        var tilesAliased = build.Manifest with
        {
            MapRegions = build.Manifest.MapRegions.Select(region =>
                region.MapId == "Tree"
                    ? region with { TileSetSha256 = main.TileSetSha256 }
                    : region).ToArray(),
        };
        File.WriteAllBytes(manifestPath, ManifestWriter.SerializeBytes(tilesAliased));
        Assert.False(MapPackValidator.IsValid(
            build.StagingPath,
            Fixture.Build,
            Hashing.Sha256Hex("contract"u8),
            Hashing.Sha256Hex("mapping"u8)));

        var metadataChanged = build.Manifest with
        {
            MapRegions = build.Manifest.MapRegions.Select(region =>
                region.MapId == "Tree"
                    ? region with { Priority = 0 }
                    : region).ToArray(),
        };
        File.WriteAllBytes(manifestPath, ManifestWriter.SerializeBytes(metadataChanged));
        Assert.False(MapPackValidator.IsValid(
            build.StagingPath,
            Fixture.Build,
            Hashing.Sha256Hex("contract"u8),
            Hashing.Sha256Hex("mapping"u8)));
    }
}
