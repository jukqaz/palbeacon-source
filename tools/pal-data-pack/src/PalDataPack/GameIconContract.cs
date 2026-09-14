using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace PalDataPack;

public sealed record GameIconGroupContract(
    [property: JsonPropertyName("group_id")] string GroupId,
    [property: JsonPropertyName("asset_root")] string AssetRoot,
    [property: JsonPropertyName("file_name_prefix")] string FileNamePrefix,
    [property: JsonPropertyName("file_name_suffix")] string FileNameSuffix,
    [property: JsonPropertyName("expected_texture_count")] int ExpectedTextureCount,
    [property: JsonPropertyName("inventory_sha256")] string InventorySha256);

public sealed record GameIconContract(
    [property: JsonPropertyName("schema_major")] int SchemaMajor,
    [property: JsonPropertyName("game_build_id")] string GameBuildId,
    [property: JsonPropertyName("reviewed")] bool Reviewed,
    [property: JsonPropertyName("review_id")] string ReviewId,
    [property: JsonPropertyName("approved_mapping_sha256")] string ApprovedMappingSha256,
    [property: JsonPropertyName("korean_pal_catalog_sha256")] string KoreanPalCatalogSha256,
    [property: JsonPropertyName("pal_icon_link_review_id")] string PalIconLinkReviewId,
    [property: JsonPropertyName("distribution_scope")] string DistributionScope,
    [property: JsonPropertyName("export_source_png")] bool ExportSourcePng,
    [property: JsonPropertyName("thumbnail_size_px")] int ThumbnailSizePx,
    [property: JsonPropertyName("thumbnail_content_size_px")] int ThumbnailContentSizePx,
    [property: JsonPropertyName("groups")] IReadOnlyList<GameIconGroupContract> Groups)
{
    public const string LocalDistributionScope = "local_windows_only";

    private const int MaximumContractBytes = 1024 * 1024;

    private static readonly JsonSerializerOptions Options = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow,
        MaxDepth = 32,
    };

    public static GameIconContract Load(string path) =>
        Parse(SecureInput.ReadBoundedRegularFile(path, MaximumContractBytes));

    public static GameIconContract Parse(string json) =>
        Parse(Encoding.UTF8.GetBytes(json));

    public static GameIconContract Parse(ReadOnlySpan<byte> bytes)
    {
        try
        {
            var contract = JsonSerializer.Deserialize<GameIconContract>(
                    bytes,
                    Options)
                ?? throw new JsonException("empty contract");
            Validate(contract);
            return contract;
        }
        catch (Exception error) when (
            error is JsonException or NotSupportedException)
        {
            throw InvalidContract();
        }
    }

    public void RequireProbe(GameIconProbeResult probe)
    {
        var expected = Groups.ToDictionary(
            group => group.GroupId,
            StringComparer.Ordinal);
        var actual = probe.Groups.ToDictionary(
            group => group.GroupId,
            StringComparer.Ordinal);
        if (!Reviewed
            || string.IsNullOrWhiteSpace(ReviewId)
            || !string.Equals(GameBuildId, probe.GameBuildId, StringComparison.Ordinal)
            || !string.Equals(
                ApprovedMappingSha256,
                probe.MappingSha256,
                StringComparison.Ordinal)
            || expected.Count != actual.Count
            || expected.Any(pair =>
                !actual.TryGetValue(pair.Key, out var observed)
                || !string.Equals(
                    pair.Value.AssetRoot,
                    observed.AssetRoot,
                    StringComparison.Ordinal)
                || !string.Equals(
                    pair.Value.FileNamePrefix,
                    observed.FileNamePrefix,
                    StringComparison.Ordinal)
                || !string.Equals(
                    pair.Value.FileNameSuffix,
                    observed.FileNameSuffix,
                    StringComparison.Ordinal)
                || pair.Value.ExpectedTextureCount != observed.TextureCount
                || !string.Equals(
                    pair.Value.InventorySha256,
                    observed.InventorySha256,
                    StringComparison.Ordinal)))
        {
            throw new DataPackFailure(
                DataPackExitCode.AssetContractMismatch,
                "game icon extraction requires the reviewed exact-Build inventory");
        }
    }

    private static void Validate(GameIconContract contract)
    {
        var definitions = GameIconProbe.Definitions.ToDictionary(
            definition => definition.GroupId,
            StringComparer.Ordinal);
        if (contract.SchemaMajor != CatalogContract.SchemaMajor
            || string.IsNullOrEmpty(contract.GameBuildId)
            || contract.GameBuildId.Any(character => !char.IsAsciiDigit(character))
            || contract.ReviewId is null
            || string.IsNullOrWhiteSpace(contract.PalIconLinkReviewId)
            || !CatalogContract.IsCanonicalSha256(contract.ApprovedMappingSha256)
            || !CatalogContract.IsCanonicalSha256(contract.KoreanPalCatalogSha256)
            || !string.Equals(
                contract.DistributionScope,
                LocalDistributionScope,
                StringComparison.Ordinal)
            || !contract.ExportSourcePng
            || contract.ThumbnailSizePx is < 32 or > 512
            || contract.ThumbnailContentSizePx is < 16
            || contract.ThumbnailContentSizePx > contract.ThumbnailSizePx
            || contract.Groups is null
            || contract.Groups.Count != definitions.Count
            || contract.Groups
                .Select(group => group.GroupId)
                .Distinct(StringComparer.Ordinal)
                .Count() != contract.Groups.Count
            || contract.Groups.Any(group =>
                group is null
                || !definitions.TryGetValue(group.GroupId, out var definition)
                || !string.Equals(
                    group.AssetRoot,
                    definition.AssetRoot,
                    StringComparison.Ordinal)
                || !string.Equals(
                    group.FileNamePrefix,
                    definition.FileNamePrefix,
                    StringComparison.Ordinal)
                || !string.Equals(
                    group.FileNameSuffix,
                    definition.FileNameSuffix,
                    StringComparison.Ordinal)
                || group.ExpectedTextureCount is < 1 or > 4096
                || !CatalogContract.IsCanonicalSha256(group.InventorySha256)))
        {
            throw InvalidContract();
        }
    }

    private static DataPackFailure InvalidContract() =>
        new(DataPackExitCode.AssetContractMismatch, "game icon contract is invalid");
}
