namespace PalDataPack;

public static class FixturePipeline
{
    public static Task<DataPackPublication> PublishAsync(
        string sourceDirectory,
        string datasetRoot,
        string expectedGameBuildId,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        var inspection = PackageValidator.InspectNormalizedSource(
            sourceDirectory,
            expectedGameBuildId);
        cancellationToken.ThrowIfCancellationRequested();
        return Task.FromResult(AtomicPublisher.Publish(
            sourceDirectory,
            datasetRoot,
            inspection));
    }
}
