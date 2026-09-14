using System.Text.Json;

namespace PalDataPack.Tests;

public sealed class PalRelationMatchBuilderTests
{
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
    };

    [Fact]
    public void Exact_build_drop_and_spawn_ids_have_reproducible_classification()
    {
        var root = TestPaths.RepositoryRoot();
        var outputDirectory = TestPaths.TempDirectory();
        try
        {
            var output = Path.Combine(outputDirectory, "relations.json");
            var result = PalRelationMatchBuilder.BuildToFile(
                Path.Combine(
                    root,
                    "assets",
            "palbeacon",
                    "game",
                    "catalog",
                    "pals_breeding.v1.json"),
                Path.Combine(
                    root,
                    "assets",
            "palbeacon",
                    "game",
                    "catalog",
                    "humans.v1.json"),
                Path.Combine(
                    root,
                    "assets",
            "palbeacon",
                    "game",
                    "catalog",
                    "items.v1.json"),
                Path.Combine(
                    root,
                    "assets",
            "palbeacon",
                    "game",
                    "map",
                    "spawns.search.v1.json"),
                output);
            var document = JsonSerializer.Deserialize<PalRelationMatchDocument>(
                File.ReadAllBytes(output),
                JsonOptions);

            Assert.NotNull(document);
            Assert.True(document.Verified);
            Assert.Equal(2, document.SchemaVersion);
            Assert.Equal("steam:24575825", document.GameBuildId);
            Assert.Equal(3_380, document.Counts.DropRelationCount);
            Assert.Equal(890, document.Counts.DropSourceIdCount);
            Assert.Equal(644, document.Counts.DropCanonicalExactCount);
            Assert.Equal(24, document.Counts.DropCanonicalCaseOnlyCount);
            Assert.Equal(18, document.Counts.DropNonPaldexSourceCount);
            Assert.Equal(176, document.Counts.DropHumanSourceCount);
            Assert.Equal(28, document.Counts.DropHumanBossAliasCount);
            Assert.Equal(0, document.Counts.DropNotInPalParameterCount);
            Assert.Equal(0, document.Counts.DropSentinelCount);
            Assert.Equal(1_829, document.Counts.SpawnRuleCount);
            Assert.Equal(483, document.Counts.SpawnSourceIdCount);
            Assert.Equal(466, document.Counts.SpawnCanonicalExactCount);
            Assert.Equal(4, document.Counts.SpawnCanonicalCaseOnlyCount);
            Assert.Equal(11, document.Counts.SpawnNonPaldexSourceCount);
            Assert.Equal(1, document.Counts.SpawnHumanSourceCount);
            Assert.Equal(0, document.Counts.SpawnHumanBossAliasCount);
            Assert.Equal(0, document.Counts.SpawnNotInPalParameterCount);
            Assert.Equal(1, document.Counts.SpawnSentinelCount);
            Assert.Equal(668, result.DropCanonicalMatchCount);
            Assert.Equal(470, result.SpawnCanonicalMatchCount);
        }
        finally
        {
            Directory.Delete(outputDirectory, recursive: true);
        }
    }

    [Fact]
    public void Relation_matching_uses_only_exact_or_unique_fname_case_links()
    {
        var root = TestPaths.RepositoryRoot();
        var outputDirectory = TestPaths.TempDirectory();
        try
        {
            var output = Path.Combine(outputDirectory, "relations.json");
            PalRelationMatchBuilder.BuildToFile(
                Path.Combine(
                    root,
                    "assets",
            "palbeacon",
                    "game",
                    "catalog",
                    "pals_breeding.v1.json"),
                Path.Combine(
                    root,
                    "assets",
            "palbeacon",
                    "game",
                    "catalog",
                    "humans.v1.json"),
                Path.Combine(
                    root,
                    "assets",
            "palbeacon",
                    "game",
                    "catalog",
                    "items.v1.json"),
                Path.Combine(
                    root,
                    "assets",
            "palbeacon",
                    "game",
                    "map",
                    "spawns.search.v1.json"),
                output);
            var document = JsonSerializer.Deserialize<PalRelationMatchDocument>(
                File.ReadAllBytes(output),
                JsonOptions)!;

            Assert.Contains(
                document.DropSources,
                match => match.SourceId == "BOSS_Anubis"
                    && match.Status == "canonical_case_only"
                    && match.MatchedSourceRowId == "Boss_Anubis"
                    && match.CanonicalInternalId == "Anubis");
            Assert.Contains(
                document.SpawnSources,
                match => match.SourceId == "Quest_farmer03_PinkCat"
                    && match.Status == "canonical_case_only"
                    && match.CanonicalInternalId == "PinkCat");
            Assert.Contains(
                document.SpawnSources,
                match => match.SourceId == "YakushimaMonster001"
                    && match.Status == "non_paldex_source_exact"
                    && match.CanonicalInternalId is null);
            Assert.Contains(
                document.SpawnSources,
                match => match.SourceId == "RowName"
                    && match.Status == "sentinel");
            Assert.Contains(
                document.DropSources,
                match => match.SourceId == "BOSS_Hunter_Bat"
                    && match.Status == "human_boss_alias_reviewed"
                    && match.MatchedSourceRowId == "Hunter_Bat"
                    && match.NameKo == "밀렵단 초짜"
                    && match.VerificationStatus == "reviewed_inference");
            Assert.Contains(
                document.SpawnSources,
                match => match.SourceId == "Male_NinjaElite01"
                    && match.Status == "human_source_exact"
                    && match.NameKo == "달꽃단 상급 닌자");
            Assert.DoesNotContain(
                document.DropSources.Concat(document.SpawnSources),
                match => match.Status == "not_in_pal_parameter");
            Assert.All(
                document.DropSources.Where(match =>
                    match.Status == "human_boss_alias_reviewed"),
                match =>
                {
                    Assert.StartsWith("BOSS_", match.SourceId);
                    Assert.Equal(
                        match.SourceId["BOSS_".Length..],
                        match.MatchedSourceRowId);
                });
            Assert.DoesNotContain(
                document.DropSources.Concat(document.SpawnSources),
                match => match.Reason.Contains(
                    "prefix",
                    StringComparison.OrdinalIgnoreCase)
                    || match.Reason.Contains(
                        "suffix",
                        StringComparison.OrdinalIgnoreCase));
        }
        finally
        {
            Directory.Delete(outputDirectory, recursive: true);
        }
    }
}
