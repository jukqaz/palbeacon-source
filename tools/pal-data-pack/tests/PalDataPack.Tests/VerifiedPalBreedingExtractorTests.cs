namespace PalDataPack.Tests;

public sealed class VerifiedPalBreedingExtractorTests
{
    [Fact]
    public void Reviewed_contract_pins_exact_Pal_breeding_and_localization_tables()
    {
        var contract = AssetContract.Load(Path.Combine(
            TestPaths.RepositoryRoot(),
            "tools",
            "pal-data-pack",
            "contracts",
            "24181527.pal-breeding-contract.json"));

        Assert.True(contract.Reviewed);
        Assert.Equal("24181527", contract.GameBuildId);
        Assert.Equal(
            "review:pal-breeding:24181527:2026-07-28",
            contract.ReviewId);
        Assert.Equal(6, contract.Tables.Count);
        Assert.Equal(
            [753, 258, 322, 310, 322, 310],
            contract.Tables
                .Select(table => table.ExpectedRowCount!.Value)
                .ToArray());
        Assert.All(contract.Tables, table =>
        {
            Assert.True(table.Required);
            Assert.NotEmpty(table.RequiredProperties);
        });
        contract.RequireExtractionIdentity(
            "24181527",
            "241c45de9d5b55b246cd4b39d62b9209faf7758ce0637e1f7a545aa0f75f71f0");
    }

    [Fact]
    public void General_formula_rounds_half_up_and_uses_duplicate_priority()
    {
        var candidates = new[]
        {
            Species("A", rank: 100, priority: 20),
            Species("B", rank: 100, priority: 10),
            Species("C", rank: 110, priority: 1),
        };

        Assert.Equal(101, VerifiedPalBreedingExtractor.GeneralTargetRank(100, 101));
        Assert.Equal(
            "A",
            VerifiedPalBreedingExtractor.GeneralChild(90, 110, candidates)
                .InternalId);
    }

    [Fact]
    public void General_formula_excludes_children_reserved_for_special_rules()
    {
        var candidates = new[]
        {
            Species("Special", rank: 100, priority: 100),
            Species("General", rank: 110, priority: 90),
        };

        Assert.Equal(
            "General",
            VerifiedPalBreedingExtractor.GeneralChild(
                    100,
                    100,
                    candidates,
                    new HashSet<string>(["Special"], StringComparer.Ordinal))
                .InternalId);
    }

    [Fact]
    public void General_formula_uses_ordinal_id_as_the_final_tie_breaker()
    {
        var candidates = new[]
        {
            Species("Zulu", rank: 90, priority: 10),
            Species("Alpha", rank: 110, priority: 10),
        };

        Assert.Equal(
            "Alpha",
            VerifiedPalBreedingExtractor.GeneralChild(100, 100, candidates)
                .InternalId);
    }

    private static VerifiedBreedingSpecies Species(
        string id,
        int rank,
        int priority) =>
        new(
            id,
            id,
            id,
            1,
            string.Empty,
            rank,
            priority,
            false,
            id);
}
