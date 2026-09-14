namespace PalDataPack.Tests;

public sealed class FullCapabilityFixtureTests
{
    [Fact]
    public async Task Synthetic_build_emits_every_required_capability()
    {
        var root = TestPaths.TempDirectory();
        try
        {
            var result = await FixturePipeline.PublishAsync(
                TestPaths.SyntheticCatalog(),
                root,
                "steam:24467282",
                CancellationToken.None);

            Assert.Equal(CatalogContract.RequiredCapabilities, result.Manifest.Capabilities
                .Select(value => value.CapabilityId));
            Assert.All(
                result.Manifest.Capabilities,
                value => Assert.Equal(CapabilityState.Complete, value.Status));
            Assert.True(File.Exists(result.ActivePointerPath));
            Assert.Equal(64, result.Manifest.DatasetManifestId.Length);
        }
        finally
        {
            Directory.Delete(root, recursive: true);
        }
    }

    [Fact]
    public async Task Missing_required_file_fails_before_publication()
    {
        var root = TestPaths.TempDirectory();
        try
        {
            var source = TestPaths.CopySyntheticCatalog(Path.Combine(root, "source"));
            File.Delete(Path.Combine(source, "items.ndjson"));

            var failure = await Assert.ThrowsAsync<DataPackFailure>(() =>
                FixturePipeline.PublishAsync(
                    source,
                    Path.Combine(root, "published"),
                    "steam:24467282",
                    CancellationToken.None));

            Assert.Equal(DataPackExitCode.SourceContractMismatch, failure.ExitCode);
            Assert.False(File.Exists(Path.Combine(
                root,
                "published",
                "steam_24467282.active.json")));
        }
        finally
        {
            Directory.Delete(root, recursive: true);
        }
    }
}
