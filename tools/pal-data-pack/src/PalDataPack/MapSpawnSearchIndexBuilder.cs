using System.Text.Json;

namespace PalDataPack;

public sealed record MapSpawnSearchIndexWriteResult(
    string GameBuildId,
    string OutputPath,
    string OutputSha256,
    int PlacementCount,
    int SpawnRuleCount,
    int SpeciesCount);

public static class MapSpawnSearchIndexBuilder
{
    private const int MaximumSourceBytes = 32 * 1024 * 1024;
    private const int MaximumOutputBytes = 8 * 1024 * 1024;

    private static readonly JsonSerializerOptions SourceJsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
    };

    private static readonly JsonSerializerOptions OutputJsonOptions = new()
    {
        WriteIndented = false,
    };

    public static MapSpawnSearchIndexWriteResult BuildToFile(
        string sourcePath,
        string outputPath)
    {
        SecureInput.EnsureRegularFile(sourcePath);
        var source = File.ReadAllBytes(sourcePath);
        if (source.Length is < 1 or > MaximumSourceBytes)
        {
            throw Failure("verified map-spawn source is outside its size bound");
        }
        var document = JsonSerializer.Deserialize<VerifiedMapSpawnDocument>(
                source,
                SourceJsonOptions)
            ?? throw Failure("verified map-spawn source is empty");
        if (document.SchemaVersion != 1
            || !document.Verified
            || !document.GameBuildId.StartsWith("steam:", StringComparison.Ordinal)
            || document.Placements.Count == 0
            || document.SpawnGroups.Count == 0)
        {
            throw Failure("verified map-spawn source identity is invalid");
        }

        var placements = document.Placements
            .Where(placement => placement.HasSpawnGroup)
            .OrderBy(placement => placement.PlacementId, StringComparer.Ordinal)
            .Select(placement => new object[]
            {
                placement.PlacementId,
                placement.SpawnerName,
                placement.WorldX,
                placement.WorldY,
                placement.WorldZ,
                placement.PlacementType,
                placement.SpawnerType,
            })
            .ToArray();
        var rulesBySpecies = new SortedDictionary<string, object[][]>(
            StringComparer.Ordinal);
        foreach (var speciesGroup in document.SpawnGroups
                     .SelectMany(group => group.Entries
                         .Where(entry => entry.PalId is not null)
                         .Select(entry => (Group: group, Entry: entry)))
                     .GroupBy(row => row.Entry.PalId!, StringComparer.Ordinal)
                     .OrderBy(group => group.Key, StringComparer.Ordinal))
        {
            rulesBySpecies.Add(
                speciesGroup.Key,
                speciesGroup
                    .OrderBy(row => row.Group.SpawnGroupId, StringComparer.Ordinal)
                    .ThenBy(row => row.Entry.Slot)
                    .Select(row => new object[]
                    {
                        row.Group.SpawnerName,
                        row.Group.SpawnerType,
                        row.Group.Weight,
                        row.Group.OnlyTime,
                        row.Group.OnlyWeather,
                        row.Group.HasWorldTreeAura,
                        row.Entry.MinimumLevel,
                        row.Entry.MaximumLevel,
                        row.Entry.MinimumCount,
                        row.Entry.MaximumCount,
                    })
                    .ToArray());
        }

        var payload = new
        {
            schema_version = 1,
            game_build_id = document.GameBuildId,
            verified = true,
            placement_unit = document.PlacementUnit,
            group_unit = document.GroupUnit,
            placement_count = document.Placements.Count,
            resolved_placement_count = placements.Length,
            spawn_group_count = document.SpawnGroups.Count,
            species_count = rulesBySpecies.Count,
            placements,
            species = rulesBySpecies,
        };
        var bytes = JsonSerializer.SerializeToUtf8Bytes(payload, OutputJsonOptions);
        if (bytes.Length is < 1 or > MaximumOutputBytes)
        {
            throw Failure("map-spawn search index is outside its size bound");
        }
        var fullOutputPath = AtomicWrite(outputPath, bytes);
        return new MapSpawnSearchIndexWriteResult(
            document.GameBuildId,
            fullOutputPath,
            Hashing.Sha256Hex(bytes),
            placements.Length,
            rulesBySpecies.Values.Sum(rows => rows.Length),
            rulesBySpecies.Count);
    }

    private static string AtomicWrite(string outputPath, ReadOnlySpan<byte> bytes)
    {
        var fullPath = Path.GetFullPath(outputPath);
        var directory = Path.GetDirectoryName(fullPath)
            ?? throw Failure("map-spawn search-index output directory is invalid");
        Directory.CreateDirectory(directory);
        SecureInput.EnsurePlainDirectory(directory);
        var temporaryPath = Path.Combine(
            directory,
            $".{Path.GetFileName(fullPath)}.{Guid.NewGuid():N}.tmp");
        try
        {
            using (var stream = new FileStream(
                temporaryPath,
                FileMode.CreateNew,
                FileAccess.Write,
                FileShare.None,
                64 * 1024,
                FileOptions.WriteThrough))
            {
                stream.Write(bytes);
                stream.Flush(flushToDisk: true);
            }
            File.Move(temporaryPath, fullPath, overwrite: true);
            return fullPath;
        }
        finally
        {
            if (File.Exists(temporaryPath))
            {
                File.Delete(temporaryPath);
            }
        }
    }

    private static DataPackFailure Failure(string message) =>
        new(DataPackExitCode.SourceContractMismatch, message);
}
