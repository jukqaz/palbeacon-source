using PalMapPack;

namespace PalMapPack.Tests;

public sealed class PoiExtractorTests
{
    [Fact]
    public void PoiExtractorEmitsOnlyV1KindsWithCompleteAccounting()
    {
        var assets = Fixture.Assets();
        var result = PoiExtractor.Extract(
            assets.PoiRows,
            assets.Regions,
            Fixture.RegionTransforms(),
            Fixture.Build);
        Assert.All(result.Pois, poi =>
            Assert.Contains(poi.Kind, new[] { "fast_travel", "boss", "wanted", "dungeon" }));
        Assert.Equal(4, result.Pois.Count);
        Assert.All(result.SourceAccounting, account =>
            Assert.Equal(account.SourceTotal, account.Extracted + account.ExplicitlyExcluded));
        Assert.All(result.Pois, poi =>
        {
            Assert.NotEmpty(poi.SourceAssetPath);
            Assert.NotEmpty(poi.SourceRowKey);
            Assert.Equal(poi.SourceRowKey, poi.SourceRowKeys[0]);
            Assert.NotEmpty(poi.ExtractionRule);
            Assert.Equal(Fixture.Build, poi.SourceBuildId);
        });
    }

    [Fact]
    public void PoiExtractorPreservesAllSemanticDeduplicationSourceRows()
    {
        var assets = Fixture.Assets();
        var rows = assets.PoiRows.Select(row => row.Kind == "FieldBoss"
            ? row with { SourceRowKeys = ["10", "20"] }
            : row);

        var result = PoiExtractor.Extract(
            rows,
            assets.Regions,
            Fixture.RegionTransforms(),
            Fixture.Build);

        var boss = Assert.Single(result.Pois, poi => poi.Kind == "boss");
        Assert.Equal("10", boss.SourceRowKey);
        Assert.Equal(new[] { "10", "20" }, boss.SourceRowKeys);
    }

    [Fact]
    public void PoiExtractorFailsOnUnaccountedUnknownRows()
    {
        var rows = Fixture.Assets().PoiRows.Append(
            new RawPoiRow("Pal/World", "unknown", "Unknown", "Unknown", 1, 2, null, true));
        var assets = Fixture.Assets();
        var error = Assert.Throws<MapPackFailure>(() =>
            PoiExtractor.Extract(rows, assets.Regions, Fixture.RegionTransforms(), Fixture.Build));
        Assert.Equal(ExitCodes.AssetContractMismatch, error.ExitCode);
    }

    [Fact]
    public void PoiExtractorFailsOnDuplicatesNonfiniteOrMissingKindCoverage()
    {
        var assets = Fixture.Assets();
        var duplicate = assets.PoiRows.Concat(new[] { assets.PoiRows[0] });
        Assert.Throws<MapPackFailure>(() =>
            PoiExtractor.Extract(duplicate, assets.Regions, Fixture.RegionTransforms(), Fixture.Build));
        Assert.Throws<MapPackFailure>(() => PoiExtractor.Extract(
            assets.PoiRows.Select(row => row.Kind == "PointFastTravel" ? row with { WorldX = double.NaN } : row),
            assets.Regions,
            Fixture.RegionTransforms(),
            Fixture.Build));
        Assert.Throws<MapPackFailure>(() => PoiExtractor.Extract(
            assets.PoiRows.Where(row => row.Kind != "FieldBoss"),
            assets.Regions,
            Fixture.RegionTransforms(),
            Fixture.Build));
    }

    [Fact]
    public void FieldBossMustBeCrossChecked()
    {
        var rows = Fixture.Assets().PoiRows
            .Select(row => row.Kind == "FieldBoss" ? row with { CrossChecked = false } : row);
        var assets = Fixture.Assets();
        Assert.Throws<MapPackFailure>(() =>
            PoiExtractor.Extract(rows, assets.Regions, Fixture.RegionTransforms(), Fixture.Build));
    }

    [Fact]
    public void OverlapCoordinateBindsPoiToTreeRegionAndTreeTransform()
    {
        var assets = Fixture.Assets();
        var rows = assets.PoiRows.Append(new RawPoiRow(
            "Pal/World",
            "tree-fast",
            "PointFastTravel",
            "Tree Fast",
            348_000,
            -600_000,
            null,
            true));

        var result = PoiExtractor.Extract(
            rows,
            assets.Regions,
            Fixture.RegionTransforms(),
            Fixture.Build);

        var poi = Assert.Single(result.Pois, item => item.Id == "tree-fast");
        Assert.Equal(("Tree", "DummyRegion"), (poi.MapId, poi.RegionId));
        Assert.InRange(poi.MapX, 0, assets.Regions.Select(348_000, -600_000).Map.Width);
        Assert.InRange(poi.MapY, 0, assets.Regions.Select(348_000, -600_000).Map.Height);
    }
}
