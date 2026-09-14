using System.Text.Json;

namespace PalDataPack;

public sealed record DataPackPublication(
    DataPackManifestV1 Manifest,
    string VersionPath,
    string ActivePointerPath);

public static class AtomicPublisher
{
    public static DataPackPublication Publish(
        string sourceDirectory,
        string datasetRoot,
        SourceInspection inspection)
    {
        Directory.CreateDirectory(datasetRoot);
        var root = Path.GetFullPath(datasetRoot);
        var buildName = CatalogContract.SafeBuildFileName(inspection.GameBuildId);
        var buildRoot = Path.Combine(root, buildName);
        var versionsRoot = Path.Combine(buildRoot, ".versions");
        Directory.CreateDirectory(versionsRoot);

        var datasetManifestId = Hashing.DatasetManifestId(
            CatalogContract.SchemaMajor,
            inspection.GameBuildId,
            "local_only",
            inspection.Sources,
            inspection.Capabilities,
            inspection.Files);
        var manifest = new DataPackManifestV1(
            CatalogContract.SchemaMajor,
            inspection.GameBuildId,
            datasetManifestId,
            "local_only",
            inspection.Sources,
            inspection.Capabilities,
            inspection.Files);
        var finalVersion = Path.Combine(versionsRoot, datasetManifestId);
        if (!Directory.Exists(finalVersion))
        {
            var staging = Path.Combine(
                buildRoot,
                $".staging-{Guid.NewGuid():N}");
            Directory.CreateDirectory(staging);
            try
            {
                foreach (var file in inspection.Files)
                {
                    CopyNewWithFlush(
                        Path.Combine(sourceDirectory, file.Path),
                        Path.Combine(staging, file.Path));
                }
                WriteNewWithFlush(
                    Path.Combine(staging, "manifest.json"),
                    ManifestCodec.Serialize(manifest));
                _ = PackageValidator.ReopenAndVerify(staging);
                Directory.Move(staging, finalVersion);
            }
            catch
            {
                if (Directory.Exists(staging))
                {
                    Directory.Delete(staging, recursive: true);
                }
                throw;
            }
        }
        var reopened = PackageValidator.ReopenAndVerify(finalVersion);
        if (reopened.DatasetManifestId != datasetManifestId)
        {
            throw new DataPackFailure(
                DataPackExitCode.Publication,
                "existing immutable version does not match candidate");
        }

        var pointerPath = Path.Combine(root, $"{buildName}.active.json");
        var temporaryPointer = Path.Combine(
            root,
            $".{buildName}.{Guid.NewGuid():N}.active.tmp");
        var pointer = JsonSerializer.SerializeToUtf8Bytes(
            new
            {
                schema_major = CatalogContract.SchemaMajor,
                game_build_id = inspection.GameBuildId,
                dataset_manifest_id = datasetManifestId,
                relative_version_path = Path.GetRelativePath(root, finalVersion)
                    .Replace('\\', '/'),
                manifest_sha256 = Hashing.Sha256File(
                    Path.Combine(finalVersion, "manifest.json")),
            },
            new JsonSerializerOptions { WriteIndented = true });
        try
        {
            WriteNewWithFlush(temporaryPointer, pointer);
            File.Move(temporaryPointer, pointerPath, overwrite: true);
        }
        finally
        {
            if (File.Exists(temporaryPointer))
            {
                File.Delete(temporaryPointer);
            }
        }
        return new DataPackPublication(reopened, finalVersion, pointerPath);
    }

    private static void CopyNewWithFlush(string source, string destination)
    {
        SecureInput.EnsureRegularFile(source);
        using var input = new FileStream(
            source,
            FileMode.Open,
            FileAccess.Read,
            FileShare.Read,
            1024 * 1024,
            FileOptions.SequentialScan);
        using var output = new FileStream(
            destination,
            FileMode.CreateNew,
            FileAccess.Write,
            FileShare.None,
            1024 * 1024,
            FileOptions.WriteThrough);
        input.CopyTo(output);
        output.Flush(flushToDisk: true);
    }

    private static void WriteNewWithFlush(string path, ReadOnlySpan<byte> bytes)
    {
        using var stream = new FileStream(
            path,
            FileMode.CreateNew,
            FileAccess.Write,
            FileShare.None,
            64 * 1024,
            FileOptions.WriteThrough);
        stream.Write(bytes);
        stream.Flush(flushToDisk: true);
    }
}
