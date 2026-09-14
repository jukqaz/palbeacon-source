using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace PalDataPack;

public sealed record ItemIconContract(
    [property: JsonPropertyName("schema_major")] int SchemaMajor,
    [property: JsonPropertyName("game_build_id")] string GameBuildId,
    [property: JsonPropertyName("reviewed")] bool Reviewed,
    [property: JsonPropertyName("review_id")] string ReviewId,
    [property: JsonPropertyName("approved_mapping_sha256")] string ApprovedMappingSha256,
    [property: JsonPropertyName("item_catalog_sha256")] string ItemCatalogSha256,
    [property: JsonPropertyName("asset_root")] string AssetRoot,
    [property: JsonPropertyName("thumbnail_size_px")] int ThumbnailSizePx,
    [property: JsonPropertyName("expected_inventory_texture_count")] int ExpectedInventoryTextureCount,
    [property: JsonPropertyName("expected_requested_icon_count")] int ExpectedRequestedIconCount,
    [property: JsonPropertyName("expected_legal_requested_icon_count")] int ExpectedLegalRequestedIconCount,
    [property: JsonPropertyName("expected_legal_match_count")] int ExpectedLegalMatchCount,
    [property: JsonPropertyName("allowed_legal_missing_icon_names")] IReadOnlyList<string> AllowedLegalMissingIconNames)
{
    private const int MaximumContractBytes = 1024 * 1024;

    private static readonly JsonSerializerOptions Options = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow,
        MaxDepth = 16,
    };

    public static ItemIconContract Load(string path) =>
        Parse(SecureInput.ReadBoundedRegularFile(path, MaximumContractBytes));

    public static ItemIconContract Parse(string json) =>
        Parse(Encoding.UTF8.GetBytes(json));

    public static ItemIconContract Parse(ReadOnlySpan<byte> bytes)
    {
        try
        {
            var contract = JsonSerializer.Deserialize<ItemIconContract>(bytes, Options)
                ?? throw new JsonException("empty contract");
            Validate(contract);
            return contract;
        }
        catch (Exception error) when (error is JsonException or NotSupportedException)
        {
            throw InvalidContract();
        }
    }

    public void RequireProbe(ItemIconProbeResult probe)
    {
        var expectedMissing = AllowedLegalMissingIconNames
            .Order(StringComparer.Ordinal)
            .ToArray();
        var actualMissing = probe.LegalMissingIconNames
            .Order(StringComparer.Ordinal)
            .ToArray();
        if (!Reviewed
            || string.IsNullOrWhiteSpace(ReviewId)
            || !string.Equals(GameBuildId, probe.GameBuildId, StringComparison.Ordinal)
            || !string.Equals(
                ApprovedMappingSha256,
                probe.MappingSha256,
                StringComparison.Ordinal)
            || !string.Equals(
                ItemCatalogSha256,
                probe.ItemCatalogSha256,
                StringComparison.Ordinal)
            || !string.Equals(AssetRoot, probe.AssetRoot, StringComparison.Ordinal)
            || ExpectedInventoryTextureCount != probe.InventoryTextureCount
            || ExpectedRequestedIconCount != probe.RequestedIconCount
            || ExpectedLegalRequestedIconCount != probe.LegalRequestedIconCount
            || ExpectedLegalMatchCount != probe.LegalMatchCount
            || probe.LegalAmbiguousIconNames.Count != 0
            || !expectedMissing.SequenceEqual(actualMissing, StringComparer.Ordinal))
        {
            throw new DataPackFailure(
                DataPackExitCode.AssetContractMismatch,
                "item icon extraction requires the reviewed exact-Build icon inventory");
        }
    }

    private static void Validate(ItemIconContract contract)
    {
        if (contract.SchemaMajor != CatalogContract.SchemaMajor
            || string.IsNullOrEmpty(contract.GameBuildId)
            || contract.GameBuildId.Any(character => !char.IsAsciiDigit(character))
            || contract.ReviewId is null
            || !CatalogContract.IsCanonicalSha256(contract.ApprovedMappingSha256)
            || !CatalogContract.IsCanonicalSha256(contract.ItemCatalogSha256)
            || !string.Equals(
                contract.AssetRoot,
                ItemIconProbe.AssetRoot,
                StringComparison.Ordinal)
            || contract.ThumbnailSizePx is < 32 or > 512
            || contract.ExpectedInventoryTextureCount is < 1 or > 4096
            || contract.ExpectedRequestedIconCount is < 1 or > 4096
            || contract.ExpectedLegalRequestedIconCount is < 1 or > 4096
            || contract.ExpectedLegalMatchCount is < 1 or > 4096
            || contract.ExpectedLegalMatchCount
                + (contract.AllowedLegalMissingIconNames?.Count ?? 0)
                != contract.ExpectedLegalRequestedIconCount
            || contract.AllowedLegalMissingIconNames is null
            || contract.AllowedLegalMissingIconNames.Any(name =>
                string.IsNullOrWhiteSpace(name)
                || name.Length > 160
                || name.Any(char.IsControl))
            || contract.AllowedLegalMissingIconNames
                .Distinct(StringComparer.Ordinal)
                .Count() != contract.AllowedLegalMissingIconNames.Count)
        {
            throw InvalidContract();
        }
    }

    private static DataPackFailure InvalidContract() =>
        new(DataPackExitCode.AssetContractMismatch, "item icon contract is invalid");
}
