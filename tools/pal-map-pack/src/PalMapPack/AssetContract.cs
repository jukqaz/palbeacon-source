using System.Text.Json;
using System.Text.Json.Serialization;

namespace PalMapPack;

public sealed record AssetSentinel(
    string PackagePath,
    string ExportClass,
    IReadOnlyList<string> RequiredProperties);

public sealed record MappingCandidateProvenance(
    string Repository,
    string Commit,
    string PublishedDate,
    long FileSizeBytes,
    string Sha256,
    string CompatibilityStatus);

public sealed record CalibrationPreviewContract(
    int WidthPx,
    int HeightPx,
    long FileSizeBytes,
    string Sha256,
    string SourceMapAssetSha256,
    string Algorithm);

public sealed record MapRegionContract(
    string MapId,
    string RegionId,
    string SourceTexturePath,
    double WorldMinX,
    double WorldMinY,
    double WorldMaxX,
    double WorldMaxY,
    double BlockSizeX,
    double BlockSizeY,
    double GridPositionX,
    double GridPositionY,
    int MapWidthPx,
    int MapHeightPx,
    int Priority,
    string? MapAssetSha256 = null,
    CalibrationPreviewContract? CalibrationPreview = null);

public sealed record PoiAssetContract(
    string WorldPackagePath,
    int ExpectedWorldExportCount,
    string FastTravelNameTablePath,
    string FastTravelLocalizedNameTablePath,
    int ExpectedFastTravelTextRowCount,
    int ExpectedFastTravelCount,
    int ExpectedDungeonCount,
    string DungeonDisplayName,
    IReadOnlyList<string> DungeonPortalExportTypes,
    string FieldBossTablePath,
    string PalNameTablePath,
    string PalLocalizedNameTablePath,
    string HumanNameTablePath,
    string HumanLocalizedNameTablePath,
    string WorldMapNameTablePath,
    string WorldMapLocalizedNameTablePath,
    int ExpectedBossRawCount,
    int ExpectedBossSemanticCount,
    int ExpectedPalBossCount,
    int ExpectedHumanBossCount,
    int ExpectedBossRegionCount,
    int ExpectedBossDuplicateCount);

public sealed record AssetContract(
    string GameBuildId,
    bool Reviewed,
    string ReviewId,
    IReadOnlyList<MapRegionContract> MapRegions,
    string WorldMapTablePath,
    PoiAssetContract PoiSources,
    IReadOnlyList<string> AllowedPointTypes,
    IReadOnlyList<AssetSentinel> Sentinels,
    MappingCandidateProvenance? MappingCandidate = null,
    string? ApprovedMappingSha256 = null,
    string? ApprovedSealedHoldoutSha256 = null)
{
    private const int MaximumContractBytes = 1 * 1024 * 1024;
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow,
    };

    public static AssetContract Load(string path)
    {
        try
        {
            var bytes = SecureFiles.ReadAllBytes(path, MaximumContractBytes);
            return Parse(bytes);
        }
        catch (Exception error) when (error is IOException
            or UnauthorizedAccessException
            or JsonException
            or MapPackFailure)
        {
            throw InvalidContract();
        }
    }

    public static AssetContract Parse(ReadOnlySpan<byte> bytes)
    {
        try
        {
            if (bytes.Length == 0 || bytes.Length > MaximumContractBytes)
            {
                throw new JsonException("asset contract size is invalid");
            }
            var contract = JsonSerializer.Deserialize<AssetContract>(bytes, JsonOptions)
                ?? throw new JsonException("empty contract");
            ValidateRequiredShape(contract);
            AssetCatalogProbe.ValidateMapRegions(contract.MapRegions);
            AssetCatalogProbe.ValidatePoiSources(contract.PoiSources);
            return contract;
        }
        catch (Exception error) when (error is JsonException or MapPackFailure)
        {
            throw InvalidContract();
        }
    }

    private static MapPackFailure InvalidContract() =>
        new(ExitCodes.AssetContractMismatch, "build asset contract is unreadable or invalid");

    public MapRegionContract RequireCalibrationCandidate(string exactGameBuildId)
    {
        if (GameBuildId != exactGameBuildId
            || Reviewed
            || !string.IsNullOrEmpty(ReviewId)
            || ApprovedMappingSha256 is not null
            || ApprovedSealedHoldoutSha256 is not null
            || MappingCandidate is null
            || string.IsNullOrWhiteSpace(MappingCandidate.Repository)
            || string.IsNullOrWhiteSpace(MappingCandidate.Commit)
            || string.IsNullOrWhiteSpace(MappingCandidate.PublishedDate)
            || MappingCandidate.FileSizeBytes <= 0
            || !CanonicalNonzeroSha256(MappingCandidate.Sha256)
            || MappingCandidate.CompatibilityStatus != "candidate_unverified"
            || MapRegions.Any(region => !CanonicalNonzeroSha256(region.MapAssetSha256)))
        {
            throw new MapPackFailure(
                ExitCodes.AssetContractMismatch,
                "calibration requires a content-bound unreviewed exact-Build candidate contract");
        }
        var mainMap = MapRegions.SingleOrDefault(region =>
            region.MapId == "MainMap" && region.RegionId == "FirstRegion");
        if (mainMap is null || !ValidCalibrationPreview(mainMap))
        {
            throw new MapPackFailure(
                ExitCodes.AssetContractMismatch,
                "candidate contract has no valid deterministic MainMap calibration preview");
        }
        return mainMap;
    }

    public string CandidateIdentitySha256()
    {
        var normalized = this with
        {
            Reviewed = false,
            ReviewId = string.Empty,
            ApprovedMappingSha256 = null,
            ApprovedSealedHoldoutSha256 = null,
        };
        return Hashing.Sha256Hex(ManifestWriter.SerializeBytes(normalized));
    }

    private static void ValidateRequiredShape(AssetContract contract)
    {
        if (string.IsNullOrWhiteSpace(contract.GameBuildId)
            || contract.GameBuildId.Any(character => !char.IsAsciiDigit(character))
            || string.IsNullOrWhiteSpace(contract.WorldMapTablePath)
            || contract.MapRegions is null
            || contract.PoiSources is null
            || contract.AllowedPointTypes is null
            || contract.Sentinels is null
            || contract.MapRegions.Any(region => region is null)
            || contract.AllowedPointTypes.Any(string.IsNullOrWhiteSpace)
            || contract.Sentinels.Any(sentinel =>
                sentinel is null
                || string.IsNullOrWhiteSpace(sentinel.PackagePath)
                || string.IsNullOrWhiteSpace(sentinel.ExportClass)
                || sentinel.RequiredProperties is null
                || sentinel.RequiredProperties.Any(string.IsNullOrWhiteSpace)
                || sentinel.RequiredProperties.Distinct(StringComparer.Ordinal).Count()
                    != sentinel.RequiredProperties.Count)
            || HasSemanticSentinelDuplicates(contract.Sentinels)
            || contract.AllowedPointTypes.Distinct(StringComparer.Ordinal).Count()
                != contract.AllowedPointTypes.Count
            || contract.PoiSources.DungeonPortalExportTypes is null)
        {
            throw new JsonException("asset contract is missing required members");
        }
    }

    private static bool HasSemanticSentinelDuplicates(
        IReadOnlyList<AssetSentinel> sentinels) =>
        sentinels
            .GroupBy(sentinel => sentinel.PackagePath, StringComparer.OrdinalIgnoreCase)
            .Any(group => group
                .Select(sentinel => sentinel.ExportClass)
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .Count() != group.Count());

    internal static bool ValidCalibrationPreview(MapRegionContract region)
    {
        var preview = region.CalibrationPreview;
        if (preview is null
            || !CanonicalNonzeroSha256(region.MapAssetSha256)
            || preview.WidthPx <= 0
            || preview.HeightPx <= 0
            || preview.WidthPx > region.MapWidthPx
            || preview.HeightPx > region.MapHeightPx
            || region.MapWidthPx % preview.WidthPx != 0
            || region.MapHeightPx % preview.HeightPx != 0
            || region.MapWidthPx / preview.WidthPx != region.MapHeightPx / preview.HeightPx
            || region.MapWidthPx / preview.WidthPx > 16
            || !CalibrationPreviewBuilder.HasExactDeterministicLayout(preview)
            || !CanonicalNonzeroSha256(preview.Sha256)
            || preview.SourceMapAssetSha256 != region.MapAssetSha256
            || preview.Algorithm != CalibrationPreviewBuilder.Algorithm)
        {
            return false;
        }
        return true;
    }

    internal static bool CanonicalNonzeroSha256(string? value) =>
        value is { Length: 64 }
        && value.Any(character => character != '0')
        && value.All(character => character is >= '0' and <= '9' or >= 'a' and <= 'f');
}

public sealed record AssetInventoryEntry(
    string PackagePath,
    string ExportClass,
    IReadOnlyList<string> Properties);

public sealed record AssetInventory(IReadOnlyList<AssetInventoryEntry> Entries);
