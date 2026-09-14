using System.Text.Json;

namespace PalDataPack.Tests;

public sealed class PalCanonicalMatchBuilderTests
{
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
    };

    [Fact]
    public void Exact_build_matching_table_is_complete_and_reproducible()
    {
        var root = TestPaths.RepositoryRoot();
        var outputDirectory = TestPaths.TempDirectory();
        try
        {
            var output = Path.Combine(outputDirectory, "matches.json");
            var result = PalCanonicalMatchBuilder.BuildToFile(
                Path.Combine(
                    root,
                    "assets",
            "palbeacon",
                    "game",
                    "catalog",
                    "pals_breeding.v1.json"),
                Path.Combine(
                    root,
                    "docs",
                    "data",
                    "GAME_ICON_MATCH_REPORT.24575825.json"),
                Path.Combine(
                    root,
                    "docs",
                    "data",
                    "PAL_ICON_SOURCE_LINKS.24575825.json"),
                output);

            var document = JsonSerializer.Deserialize<PalCanonicalMatchDocument>(
                File.ReadAllBytes(output),
                JsonOptions);
            Assert.NotNull(document);
            Assert.Equal(288, result.CanonicalPalCount);
            Assert.Equal(288, result.FullyMatchedPalCount);
            Assert.Equal(753, document.Counts.SourcePalRowCount);
            Assert.Equal(288, document.Counts.BreedingSpeciesCount);
            Assert.Equal(257, document.Counts.SpecialBreedingRuleCount);
            Assert.Equal(
                184,
                document.Counts.PaldexSpecialBreedingRuleCount);
            Assert.Equal(0, document.Counts.CanonicalPalWithoutIconCount);
            Assert.All(
                document.Matches,
                match => Assert.Equal("fully_matched", match.MatchStatus));
            Assert.All(
                document.Matches,
                match => Assert.NotEmpty(match.SourceRows));
        }
        finally
        {
            Directory.Delete(outputDirectory, recursive: true);
        }
    }

    [Fact]
    public void Matching_table_keeps_unprojected_rows_and_icon_gaps_explicit()
    {
        var root = TestPaths.RepositoryRoot();
        var outputDirectory = TestPaths.TempDirectory();
        try
        {
            var output = Path.Combine(outputDirectory, "matches.json");
            PalCanonicalMatchBuilder.BuildToFile(
                Path.Combine(
                    root,
                    "assets",
            "palbeacon",
                    "game",
                    "catalog",
                    "pals_breeding.v1.json"),
                Path.Combine(
                    root,
                    "docs",
                    "data",
                    "GAME_ICON_MATCH_REPORT.24575825.json"),
                Path.Combine(
                    root,
                    "docs",
                    "data",
                    "PAL_ICON_SOURCE_LINKS.24575825.json"),
                output);
            var document = JsonSerializer.Deserialize<PalCanonicalMatchDocument>(
                File.ReadAllBytes(output),
                JsonOptions)!;

            Assert.Contains(
                document.Unmatched,
                match => match.Kind == "source_row_outside_paldex");
            Assert.Contains(
                document.Unmatched,
                match => match.Kind == "localized_name_without_icon"
                    && match.SubjectId == "RAID_YakushimaBoss002");
            Assert.Contains(
                document.Unmatched,
                match => match.Kind == "icon_without_korean_localization");
            Assert.DoesNotContain(
                document.Matches,
                match => match.Localization.NameKo == "ko_Text");
        }
        finally
        {
            Directory.Delete(outputDirectory, recursive: true);
        }
    }
}
