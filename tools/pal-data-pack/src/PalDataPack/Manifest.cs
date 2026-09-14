using System.Text.Json;
using System.Text.Json.Serialization;

namespace PalDataPack;

public enum CapabilityState
{
    Complete,
    UnsupportedByBuild,
    Failed,
}

public static class CapabilityStateExtensions
{
    public static string ToWireValue(this CapabilityState state) => state switch
    {
        CapabilityState.Complete => "complete",
        CapabilityState.UnsupportedByBuild => "unsupported_by_build",
        CapabilityState.Failed => "failed",
        _ => throw new ArgumentOutOfRangeException(nameof(state)),
    };

    public static CapabilityState ParseWireValue(string value) => value switch
    {
        "complete" => CapabilityState.Complete,
        "unsupported_by_build" => CapabilityState.UnsupportedByBuild,
        "failed" => CapabilityState.Failed,
        _ => throw new DataPackFailure(
            DataPackExitCode.PackageIntegrity,
            "manifest capability state is invalid"),
    };
}

public sealed record SourceFingerprint(
    [property: JsonPropertyName("source_id")] string SourceId,
    [property: JsonPropertyName("source_kind")] string SourceKind,
    [property: JsonPropertyName("source_sha256")] string SourceSha256,
    [property: JsonPropertyName("verified")] bool Verified);

public sealed record CapabilityStatus(
    [property: JsonPropertyName("capability_id")] string CapabilityId,
    [property: JsonIgnore] CapabilityState Status,
    [property: JsonPropertyName("source_ids")] IReadOnlyList<string> SourceIds)
{
    [JsonPropertyName("status")]
    public string StatusValue => Status.ToWireValue();
}

public sealed record PackageFile(
    [property: JsonPropertyName("path")] string Path,
    [property: JsonPropertyName("row_count")] long RowCount,
    [property: JsonPropertyName("file_size_bytes")] long FileSizeBytes,
    [property: JsonPropertyName("sha256")] string Sha256);

public sealed record DataPackManifestV1(
    [property: JsonPropertyName("schema_major")] int SchemaMajor,
    [property: JsonPropertyName("game_build_id")] string GameBuildId,
    [property: JsonPropertyName("dataset_manifest_id")] string DatasetManifestId,
    [property: JsonPropertyName("distribution_scope")] string DistributionScope,
    [property: JsonPropertyName("sources")] IReadOnlyList<SourceFingerprint> Sources,
    [property: JsonPropertyName("capabilities")] IReadOnlyList<CapabilityStatus> Capabilities,
    [property: JsonPropertyName("files")] IReadOnlyList<PackageFile> Files);

public static class ManifestCodec
{
    private const int MaximumManifestBytes = 1024 * 1024;

    private static readonly JsonSerializerOptions WriteOptions = new()
    {
        WriteIndented = true,
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        DefaultIgnoreCondition = JsonIgnoreCondition.Never,
    };

    public static byte[] Serialize(DataPackManifestV1 manifest)
    {
        var document = new
        {
            schema_major = manifest.SchemaMajor,
            game_build_id = manifest.GameBuildId,
            dataset_manifest_id = manifest.DatasetManifestId,
            distribution_scope = manifest.DistributionScope,
            sources = manifest.Sources.Select(source => new
            {
                source_id = source.SourceId,
                source_kind = source.SourceKind,
                source_sha256 = source.SourceSha256,
                verified = source.Verified,
            }),
            capabilities = manifest.Capabilities.Select(capability => new
            {
                capability_id = capability.CapabilityId,
                status = capability.Status.ToWireValue(),
                source_ids = capability.SourceIds,
            }),
            files = manifest.Files.Select(file => new
            {
                path = file.Path,
                row_count = file.RowCount,
                file_size_bytes = file.FileSizeBytes,
                sha256 = file.Sha256,
            }),
        };
        return JsonSerializer.SerializeToUtf8Bytes(document, WriteOptions);
    }

    public static DataPackManifestV1 Load(string path)
    {
        var bytes = SecureInput.ReadBoundedRegularFile(path, MaximumManifestBytes);
        try
        {
            using var document = JsonDocument.Parse(bytes, new JsonDocumentOptions
            {
                AllowTrailingCommas = false,
                CommentHandling = JsonCommentHandling.Disallow,
                MaxDepth = 32,
            });
            var root = document.RootElement;
            var sources = root.GetProperty("sources").EnumerateArray().Select(source =>
                new SourceFingerprint(
                    source.GetProperty("source_id").GetString()!,
                    source.GetProperty("source_kind").GetString()!,
                    source.GetProperty("source_sha256").GetString()!,
                    source.GetProperty("verified").GetBoolean())).ToArray();
            var capabilities = root.GetProperty("capabilities").EnumerateArray().Select(capability =>
                new CapabilityStatus(
                    capability.GetProperty("capability_id").GetString()!,
                    CapabilityStateExtensions.ParseWireValue(
                        capability.GetProperty("status").GetString()!),
                    capability.GetProperty("source_ids").EnumerateArray()
                        .Select(value => value.GetString()!)
                        .ToArray())).ToArray();
            var files = root.GetProperty("files").EnumerateArray().Select(file =>
                new PackageFile(
                    file.GetProperty("path").GetString()!,
                    file.GetProperty("row_count").GetInt64(),
                    file.GetProperty("file_size_bytes").GetInt64(),
                    file.GetProperty("sha256").GetString()!)).ToArray();
            return new DataPackManifestV1(
                root.GetProperty("schema_major").GetInt32(),
                root.GetProperty("game_build_id").GetString()!,
                root.GetProperty("dataset_manifest_id").GetString()!,
                root.GetProperty("distribution_scope").GetString()!,
                sources,
                capabilities,
                files);
        }
        catch (Exception error) when (error is JsonException or InvalidOperationException)
        {
            throw new DataPackFailure(
                DataPackExitCode.PackageIntegrity,
                "manifest is malformed");
        }
    }
}
