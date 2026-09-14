using PalMapPack;

namespace PalMapPack.Tests;

public sealed class BossPoiTableReaderTests
{
    [Fact]
    public void NormalizeRequiresExactFourPropertySchemaAndTypes()
    {
        var valid = Row("0", "pal-spawner", "BOSS_Horus_Water", 1, 2, 3, 66);
        var extra = valid.Properties.ToDictionary(pair => pair.Key, pair => pair.Value, StringComparer.Ordinal);
        extra.Add("Unexpected", new BossPoiIntegerValue(1));
        var missing = valid.Properties
            .Where(pair => pair.Key != "Level")
            .ToDictionary(pair => pair.Key, pair => pair.Value, StringComparer.Ordinal);
        var wrongType = valid.Properties.ToDictionary(pair => pair.Key, pair => pair.Value, StringComparer.Ordinal);
        wrongType["Location"] = new BossPoiNameValue("not-a-vector");

        AssertContractFailure(new BossPoiSourceRow("extra", extra));
        AssertContractFailure(new BossPoiSourceRow("missing", missing));
        AssertContractFailure(new BossPoiSourceRow("wrong-type", wrongType));

        var result = BossPoiTableReader.Normalize([valid], new PrefixNameResolver("ko"));
        Assert.Single(result.Pois);
    }

    [Fact]
    public void NormalizeClassifiesThreeFamiliesAndUsesExactLocalizationKeys()
    {
        var resolver = new PrefixNameResolver("ko");
        var result = BossPoiTableReader.Normalize(
            [
                Row("0", "81_1_grass_FBOSS_14", "Boss_Anubis", 10, 20, 30, 55),
                Row("1", "BOSS_DarkTrader", "None", 40, 50, 60, 59),
                Row("2", "REGION_Oilrig_1", "None", 70, 80, 90, 55),
            ],
            resolver);

        var pal = Assert.Single(result.Pois, poi => poi.Kind == BossPoiKind.PalFieldBoss);
        Assert.Equal(new BossPoiNameLookup(BossPoiNameTable.Pal, "PAL_NAME_Anubis"), pal.NameLookup);
        Assert.Equal("ko:Pal:PAL_NAME_Anubis", pal.DisplayName);

        var human = Assert.Single(result.Pois, poi => poi.Kind == BossPoiKind.HumanFieldBoss);
        Assert.Equal(new BossPoiNameLookup(BossPoiNameTable.Human, "NAME_BOSS_DarkTrader"), human.NameLookup);
        Assert.Equal("ko:Human:NAME_BOSS_DarkTrader", human.DisplayName);

        var region = Assert.Single(result.Pois, poi => poi.Kind == BossPoiKind.BossRegion);
        Assert.Equal(new BossPoiNameLookup(BossPoiNameTable.WorldMap, "REGION_Oilrig_1"), region.NameLookup);
        Assert.Equal("ko:WorldMap:REGION_Oilrig_1", region.DisplayName);
    }

    [Fact]
    public void NormalizeDeduplicatesOnlyExactSemanticRowsAndRetainsAllRawRowKeys()
    {
        var rows = new[]
        {
            Row("20", "BOSS_DarkTrader", "None", -666_737.688, -409_956.375, 1_000, 59),
            Row("10", "BOSS_DarkTrader", "None", -666_737.688, -409_956.375, 1_000, 59),
        };

        var result = BossPoiTableReader.Normalize(rows, new PrefixNameResolver("ko"));

        Assert.Equal(2, result.RawRowCount);
        Assert.Equal(1, result.DuplicateRowCount);
        var poi = Assert.Single(result.Pois);
        Assert.Equal(new[] { "10", "20" }, poi.RawRowKeys);
    }

    [Fact]
    public void LogicalIdIsIndependentOfLocaleLevelAndRawRowIndex()
    {
        var korean = BossPoiTableReader.Normalize(
            [Row("0", "81_1_grass_FBOSS_14", "Boss_Anubis", -1.25, 2.5, -0.0, 55)],
            new PrefixNameResolver("ko"));
        var english = BossPoiTableReader.Normalize(
            [Row("158", "81_1_grass_FBOSS_14", "Boss_Anubis", -1.25, 2.5, 0.0, 79)],
            new PrefixNameResolver("en"));

        var koreanPoi = Assert.Single(korean.Pois);
        var englishPoi = Assert.Single(english.Pois);
        Assert.Equal(koreanPoi.LogicalId, englishPoi.LogicalId);
        Assert.NotEqual(koreanPoi.DisplayName, englishPoi.DisplayName);
        Assert.NotEqual(koreanPoi.Level, englishPoi.Level);
        Assert.Matches("^boss:[0-9a-f]{64}$", koreanPoi.LogicalId);
    }

    [Fact]
    public void NormalizeFailsClosedForUnknownFamilyMissingNameAndStableIdCollision()
    {
        AssertContractFailure(
            Row("0", "UNKNOWN_SPAWNER", "None", 1, 2, 3, 10));

        var missingName = new PrefixNameResolver("ko") { ReturnMissing = true };
        var missingNameError = Assert.Throws<MapPackFailure>(() => BossPoiTableReader.Normalize(
            [Row("0", "pal-spawner", "BOSS_Horus_Water", 1, 2, 3, 66)],
            missingName));
        Assert.Equal(ExitCodes.AssetContractMismatch, missingNameError.ExitCode);

        var conflictingLevels = new[]
        {
            Row("0", "pal-spawner", "BOSS_Horus_Water", 1, 2, 3, 65),
            Row("1", "pal-spawner", "BOSS_Horus_Water", 1, 2, 3, 66),
        };
        var collision = Assert.Throws<MapPackFailure>(() => BossPoiTableReader.Normalize(
            conflictingLevels,
            new PrefixNameResolver("ko")));
        Assert.Equal(ExitCodes.AssetContractMismatch, collision.ExitCode);
    }

    [Fact]
    public void SyntheticActualTableShapeNormalizes159RowsTo126SemanticPois()
    {
        var rows = new List<BossPoiSourceRow>(159);
        var rowKey = 0;
        for (var index = 0; index < 90; index++)
        {
            rows.Add(Row(
                (rowKey++).ToString(System.Globalization.CultureInfo.InvariantCulture),
                $"PAL_SPAWNER_{index}",
                $"BOSS_Pal_{index}",
                index * 100,
                index * -100,
                index,
                10 + index % 70));
        }
        for (var index = 0; index < 33; index++)
        {
            var first = Row(
                (rowKey++).ToString(System.Globalization.CultureInfo.InvariantCulture),
                $"BOSS_Human_{index}",
                "None",
                100_000 + index,
                200_000 + index,
                300 + index,
                20 + index);
            rows.Add(first);
            rows.Add(first with
            {
                RawRowKey = (rowKey++).ToString(System.Globalization.CultureInfo.InvariantCulture),
            });
        }
        for (var index = 0; index < 3; index++)
        {
            rows.Add(Row(
                (rowKey++).ToString(System.Globalization.CultureInfo.InvariantCulture),
                $"REGION_Oilrig_{index + 1}",
                "None",
                300_000 + index,
                400_000 + index,
                -2_000,
                30 + index));
        }

        var result = BossPoiTableReader.Normalize(rows, new PrefixNameResolver("fixture"));

        Assert.Equal(159, rows.Count);
        Assert.Equal(159, result.RawRowCount);
        Assert.Equal(33, result.DuplicateRowCount);
        Assert.Equal(126, result.Pois.Count);
        Assert.Equal(90, result.Pois.Count(poi => poi.Kind == BossPoiKind.PalFieldBoss));
        Assert.Equal(33, result.Pois.Count(poi => poi.Kind == BossPoiKind.HumanFieldBoss));
        Assert.Equal(3, result.Pois.Count(poi => poi.Kind == BossPoiKind.BossRegion));
        Assert.Equal(159, result.Pois.Sum(poi => poi.RawRowKeys.Count));
        Assert.Equal(126, result.Pois.Select(poi => poi.LogicalId).Distinct(StringComparer.Ordinal).Count());
    }

    private static BossPoiSourceRow Row(
        string rowKey,
        string spawnerId,
        string characterId,
        double x,
        double y,
        double z,
        int level) =>
        new(
            rowKey,
            new Dictionary<string, BossPoiSourceValue>(StringComparer.Ordinal)
            {
                ["SpawnerID"] = new BossPoiNameValue(spawnerId),
                ["CharacterID"] = new BossPoiNameValue(characterId),
                ["Location"] = new BossPoiVectorValue(x, y, z),
                ["Level"] = new BossPoiIntegerValue(level),
            });

    private static void AssertContractFailure(BossPoiSourceRow row)
    {
        var error = Assert.Throws<MapPackFailure>(() => BossPoiTableReader.Normalize(
            [row],
            new PrefixNameResolver("ko")));
        Assert.Equal(ExitCodes.AssetContractMismatch, error.ExitCode);
    }

    private sealed class PrefixNameResolver(string locale) : IBossPoiLocalizedNameResolver
    {
        public bool ReturnMissing { get; init; }

        public string? Resolve(BossPoiNameTable table, string key) =>
            ReturnMissing ? null : $"{locale}:{table}:{key}";
    }
}
