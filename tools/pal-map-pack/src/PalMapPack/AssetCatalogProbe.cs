namespace PalMapPack;

public static class AssetCatalogProbe
{
    public static void EnsureContractAccepted(
        AssetContract contract,
        string actualBuildId,
        AssetInventory inventory)
    {
        if (contract.GameBuildId != actualBuildId)
        {
            throw Failure("asset contract Build does not match the installed Steam Build");
        }
        if (!contract.Reviewed || string.IsNullOrWhiteSpace(contract.ReviewId))
        {
            throw Failure("asset contract is candidate-only and has not been reviewed/accepted");
        }
        if (!CanonicalNonzeroHash(contract.ApprovedSealedHoldoutSha256))
        {
            throw Failure("reviewed asset contract has no approved sealed-holdout commitment");
        }
        if (contract.AllowedPointTypes.Order(StringComparer.Ordinal).SequenceEqual(
                new[] { "FieldBoss", "PointDungeonPortal", "PointFastTravel" }) is false)
        {
            throw Failure("asset contract must allow exactly the three v1 POI source classes");
        }
        if (contract.Sentinels.Count == 0)
        {
            throw Failure("asset contract contains no sentinel exports");
        }
        ValidateMapRegions(contract.MapRegions);
        if (contract.MapRegions.Any(region =>
                !AssetContract.CanonicalNonzeroSha256(region.MapAssetSha256))
            || contract.MapRegions.Single(region =>
                region.MapId == "MainMap" && region.RegionId == "FirstRegion")
                .CalibrationPreview is null)
        {
            throw Failure("reviewed asset contract is not bound to exact map content and preview");
        }
        ValidatePoiSources(contract.PoiSources);

        foreach (var sentinel in contract.Sentinels)
        {
            var match = inventory.Entries.SingleOrDefault(entry =>
                entry.PackagePath == sentinel.PackagePath
                && entry.ExportClass == sentinel.ExportClass);
            if (match is null)
            {
                throw Failure("asset contract sentinel export is missing");
            }
            var actualProperties = match.Properties.Order(StringComparer.Ordinal);
            var expectedProperties = sentinel.RequiredProperties.Order(StringComparer.Ordinal);
            if (!actualProperties.SequenceEqual(expectedProperties))
            {
                throw Failure("asset contract sentinel property shape changed");
            }
        }
    }

    internal static void ValidateMapRegions(IReadOnlyList<MapRegionContract> regions)
    {
        if (regions.Count == 0
            || regions.Select(region => $"{region.MapId}\0{region.RegionId}")
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .Count() != regions.Count
            || regions.Count(region =>
                region.MapId == "MainMap" && region.RegionId == "FirstRegion") != 1
            || regions.Any(region =>
                string.IsNullOrWhiteSpace(region.MapId)
                || string.IsNullOrWhiteSpace(region.RegionId)
                || string.IsNullOrWhiteSpace(region.SourceTexturePath)
                || !double.IsFinite(region.WorldMinX)
                || !double.IsFinite(region.WorldMinY)
                || !double.IsFinite(region.WorldMaxX)
                || !double.IsFinite(region.WorldMaxY)
                || region.WorldMinX >= region.WorldMaxX
                || region.WorldMinY >= region.WorldMaxY
                || !double.IsFinite(region.BlockSizeX)
                || !double.IsFinite(region.BlockSizeY)
                || region.BlockSizeX <= 0
                || region.BlockSizeY <= 0
                || !double.IsFinite(region.GridPositionX)
                || !double.IsFinite(region.GridPositionY)
                || region.MapWidthPx is <= 0 or > 8192
                || region.MapHeightPx is <= 0 or > 8192
                || (region.MapAssetSha256 is not null
                    && !AssetContract.CanonicalNonzeroSha256(region.MapAssetSha256))
                || (region.CalibrationPreview is not null
                    && !AssetContract.ValidCalibrationPreview(region))))
        {
            throw Failure("asset contract map region inventory is invalid");
        }
    }

    internal static void ValidatePoiSources(PoiAssetContract sources)
    {
        var paths = new[]
        {
            sources.WorldPackagePath,
            sources.FastTravelNameTablePath,
            sources.FastTravelLocalizedNameTablePath,
            sources.FieldBossTablePath,
            sources.PalNameTablePath,
            sources.PalLocalizedNameTablePath,
            sources.HumanNameTablePath,
            sources.HumanLocalizedNameTablePath,
            sources.WorldMapNameTablePath,
            sources.WorldMapLocalizedNameTablePath,
        };
        if (paths.Any(string.IsNullOrWhiteSpace)
            || string.IsNullOrWhiteSpace(sources.DungeonDisplayName)
            || sources.ExpectedWorldExportCount <= 0
            || sources.ExpectedFastTravelCount <= 0
            || sources.ExpectedDungeonCount <= 0
            || sources.ExpectedFastTravelTextRowCount < sources.ExpectedFastTravelCount
            || sources.ExpectedBossRawCount <= 0
            || sources.ExpectedBossSemanticCount <= 0
            || sources.ExpectedPalBossCount <= 0
            || sources.ExpectedHumanBossCount <= 0
            || sources.ExpectedBossRegionCount <= 0
            || sources.ExpectedBossDuplicateCount < 0
            || sources.ExpectedBossRawCount
                != sources.ExpectedBossSemanticCount + sources.ExpectedBossDuplicateCount
            || sources.ExpectedBossSemanticCount
                != sources.ExpectedPalBossCount
                    + sources.ExpectedHumanBossCount
                    + sources.ExpectedBossRegionCount
            || sources.DungeonPortalExportTypes.Count == 0
            || sources.DungeonPortalExportTypes.Any(string.IsNullOrWhiteSpace)
            || sources.DungeonPortalExportTypes
                .Distinct(StringComparer.Ordinal)
                .Count() != sources.DungeonPortalExportTypes.Count)
        {
            throw Failure("asset contract POI source inventory is invalid");
        }
    }

    private static MapPackFailure Failure(string message) =>
        new(ExitCodes.AssetContractMismatch, message);

    private static bool CanonicalNonzeroHash(string? value) =>
        value is { Length: 64 }
        && value.Any(character => character != '0')
        && value.All(character => character is >= '0' and <= '9' or >= 'a' and <= 'f');
}
