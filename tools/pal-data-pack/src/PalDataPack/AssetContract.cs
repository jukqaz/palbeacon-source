using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace PalDataPack;

public sealed record AssetTableContract(
    [property: JsonPropertyName("capability")] string Capability,
    [property: JsonPropertyName("package_path")] string PackagePath,
    [property: JsonPropertyName("required")] bool Required,
    [property: JsonPropertyName("required_properties")] IReadOnlyList<string> RequiredProperties,
    [property: JsonPropertyName("row_struct")] string? RowStruct = null,
    [property: JsonPropertyName("expected_row_count")] int? ExpectedRowCount = null);

public sealed record AssetContract(
    [property: JsonPropertyName("schema_major")] int SchemaMajor,
    [property: JsonPropertyName("game_build_id")] string GameBuildId,
    [property: JsonPropertyName("reviewed")] bool Reviewed,
    [property: JsonPropertyName("review_id")] string ReviewId,
    [property: JsonPropertyName("approved_mapping_sha256")] string ApprovedMappingSha256,
    [property: JsonPropertyName("tables")] IReadOnlyList<AssetTableContract> Tables,
    [property: JsonPropertyName("expected_output_sha256")] string? ExpectedOutputSha256 = null)
{
    private const int MaximumContractBytes = 1024 * 1024;

    private static readonly JsonSerializerOptions Options = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow,
        MaxDepth = 32,
    };

    public static AssetContract Load(string path) =>
        Parse(SecureInput.ReadBoundedRegularFile(path, MaximumContractBytes));

    public static AssetContract Parse(string json) =>
        Parse(Encoding.UTF8.GetBytes(json));

    public static AssetContract Parse(ReadOnlySpan<byte> bytes)
    {
        try
        {
            var contract = JsonSerializer.Deserialize<AssetContract>(bytes, Options)
                ?? throw new JsonException("empty contract");
            Validate(contract);
            return contract;
        }
        catch (Exception error) when (error is JsonException or NotSupportedException)
        {
            throw InvalidContract();
        }
    }

    public void RequireExtractionIdentity(string actualBuildId, string mappingSha256)
    {
        if (!Reviewed
            || string.IsNullOrWhiteSpace(ReviewId)
            || !string.Equals(GameBuildId, actualBuildId, StringComparison.Ordinal)
            || !string.Equals(ApprovedMappingSha256, mappingSha256, StringComparison.Ordinal))
        {
            throw new DataPackFailure(
                DataPackExitCode.AssetContractMismatch,
                "extraction requires a reviewed exact-Build contract and approved mapping");
        }
    }

    public void RequireProbeIdentity(string actualBuildId, string mappingSha256)
    {
        if (!string.Equals(GameBuildId, actualBuildId, StringComparison.Ordinal)
            || !string.Equals(ApprovedMappingSha256, mappingSha256, StringComparison.Ordinal))
        {
            throw new DataPackFailure(
                DataPackExitCode.AssetContractMismatch,
                "probe inputs do not match the exact-Build contract");
        }
    }

    private static void Validate(AssetContract contract)
    {
        if (contract.SchemaMajor != CatalogContract.SchemaMajor
            || string.IsNullOrEmpty(contract.GameBuildId)
            || contract.GameBuildId.Any(character => !char.IsAsciiDigit(character))
            || contract.ReviewId is null
            || !CatalogContract.IsCanonicalSha256(contract.ApprovedMappingSha256)
            || (contract.ExpectedOutputSha256 is not null
                && !CatalogContract.IsCanonicalSha256(contract.ExpectedOutputSha256))
            || contract.Tables is null
            || contract.Tables.Count is 0 or > 128
            || contract.Tables.Any(table =>
                table is null
                || !CatalogContract.RequiredCapabilities.Contains(
                    table.Capability,
                    StringComparer.Ordinal)
                || string.IsNullOrWhiteSpace(table.PackagePath)
                || !table.PackagePath.StartsWith("Pal/Content/", StringComparison.Ordinal)
                || table.PackagePath.Contains("..", StringComparison.Ordinal)
                || table.RequiredProperties is null
                || table.RequiredProperties.Any(string.IsNullOrWhiteSpace)
                || table.RequiredProperties.Distinct(StringComparer.Ordinal).Count()
                    != table.RequiredProperties.Count
                || table.ExpectedRowCount is <= 0 or > 1_048_576)
            || contract.Tables
                .Select(table => table.PackagePath)
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .Count() != contract.Tables.Count)
        {
            throw InvalidContract();
        }
    }

    private static DataPackFailure InvalidContract() =>
        new(DataPackExitCode.AssetContractMismatch, "asset contract is invalid");
}
