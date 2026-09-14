namespace PalDataPack.Tests;

public sealed class AtomicPublishTests
{
    [Fact]
    public async Task Same_content_reuses_identical_immutable_version()
    {
        var root = TestPaths.TempDirectory();
        try
        {
            var first = await FixturePipeline.PublishAsync(
                TestPaths.SyntheticCatalog(),
                root,
                "steam:24467282",
                CancellationToken.None);
            var second = await FixturePipeline.PublishAsync(
                TestPaths.SyntheticCatalog(),
                root,
                "steam:24467282",
                CancellationToken.None);

            Assert.Equal(
                first.Manifest.DatasetManifestId,
                second.Manifest.DatasetManifestId);
            Assert.Equal(first.VersionPath, second.VersionPath);
            Assert.Single(Directory.EnumerateDirectories(
                Path.GetDirectoryName(first.VersionPath)!));
        }
        finally
        {
            Directory.Delete(root, recursive: true);
        }
    }

    [Fact]
    public async Task Failed_candidate_does_not_replace_active_pointer()
    {
        var root = TestPaths.TempDirectory();
        try
        {
            var first = await FixturePipeline.PublishAsync(
                TestPaths.SyntheticCatalog(),
                root,
                "steam:24467282",
                CancellationToken.None);
            var pointerBefore = await File.ReadAllBytesAsync(first.ActivePointerPath);

            var invalid = TestPaths.CopySyntheticCatalog(Path.Combine(root, "invalid"));
            File.Delete(Path.Combine(invalid, "skills.ndjson"));
            await Assert.ThrowsAsync<DataPackFailure>(() =>
                FixturePipeline.PublishAsync(
                    invalid,
                    root,
                    "steam:24467282",
                    CancellationToken.None));

            Assert.Equal(
                pointerBefore,
                await File.ReadAllBytesAsync(first.ActivePointerPath));
        }
        finally
        {
            Directory.Delete(root, recursive: true);
        }
    }
}
