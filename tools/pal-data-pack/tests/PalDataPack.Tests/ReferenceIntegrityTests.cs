namespace PalDataPack.Tests;

public sealed class ReferenceIntegrityTests
{
    [Fact]
    public async Task Unknown_recipe_item_is_rejected()
    {
        var root = TestPaths.TempDirectory();
        try
        {
            var source = TestPaths.CopySyntheticCatalog(Path.Combine(root, "source"));
            var recipePath = Path.Combine(source, "recipes.ndjson");
            var recipe = await File.ReadAllTextAsync(recipePath);
            await File.WriteAllTextAsync(
                recipePath,
                recipe.Replace("FixtureIngredient", "MissingItem", StringComparison.Ordinal));

            var failure = await Assert.ThrowsAsync<DataPackFailure>(() =>
                FixturePipeline.PublishAsync(
                    source,
                    Path.Combine(root, "published"),
                    "steam:24467282",
                    CancellationToken.None));

            Assert.Equal(DataPackExitCode.ReferenceIntegrity, failure.ExitCode);
        }
        finally
        {
            Directory.Delete(root, recursive: true);
        }
    }

    [Fact]
    public async Task Published_file_mutation_is_detected()
    {
        var root = TestPaths.TempDirectory();
        try
        {
            var result = await FixturePipeline.PublishAsync(
                TestPaths.SyntheticCatalog(),
                root,
                "steam:24467282",
                CancellationToken.None);
            var pals = Path.Combine(result.VersionPath, "pals.ndjson");
            await File.AppendAllTextAsync(pals, " ");

            var failure = Assert.Throws<DataPackFailure>(() =>
                PackageValidator.ReopenAndVerify(result.VersionPath));

            Assert.Equal(DataPackExitCode.PackageIntegrity, failure.ExitCode);
        }
        finally
        {
            Directory.Delete(root, recursive: true);
        }
    }
}
