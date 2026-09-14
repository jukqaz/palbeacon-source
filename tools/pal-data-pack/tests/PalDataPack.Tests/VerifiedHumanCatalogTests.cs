using System.Text.Json;

namespace PalDataPack.Tests;

public sealed class VerifiedHumanCatalogTests
{
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
    };

    [Fact]
    public void Reviewed_human_catalog_has_exact_build_names_and_icons()
    {
        var root = TestPaths.RepositoryRoot();
        var path = Path.Combine(
            root,
            "assets",
            "palbeacon",
            "game",
            "catalog",
            "humans.v1.json");
        var document = JsonSerializer.Deserialize<VerifiedHumanCatalogDocument>(
            File.ReadAllBytes(path),
            JsonOptions);

        Assert.NotNull(document);
        Assert.True(document.Verified);
        Assert.Equal("steam:24575825", document.GameBuildId);
        Assert.Equal(
            "review:human-relations:24575825:2026-08-14",
            document.ContractReviewId);
        Assert.Equal(5, document.Sources.Count);
        Assert.Equal(433, document.Humans.Count);
        Assert.Equal(
            433,
            document.Humans
                .Select(human => human.SourceRowId)
                .Distinct(StringComparer.Ordinal)
                .Count());
        Assert.Equal(
            398,
            document.Humans.Count(human => human.NameKo is not null));
        Assert.Equal(
            369,
            document.Humans.Count(human =>
                human.IconPackagePath is not null));
        Assert.Contains(
            document.Humans,
            human => human.SourceRowId == "Hunter_Bat"
                && human.LocalizationKey == "NAME_HUNTER"
                && human.NameKo == "밀렵단 초짜"
                && human.NameEn == "Syndicate Thug"
                && human.IconPackagePath is not null);
        Assert.Contains(
            document.Humans,
            human => human.SourceRowId == "QuestMan"
                && human.NameKo is null);
        Assert.DoesNotContain(
            document.Humans,
            human => human.NameKo == "ko_Text"
                || human.NameEn == "en_Text");
    }
}
