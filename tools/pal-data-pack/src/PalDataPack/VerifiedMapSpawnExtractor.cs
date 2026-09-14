using System.Globalization;
using System.Text.Json;
using CUE4Parse.FileProvider;
using CUE4Parse.UE4.Assets.Exports.Engine;
using CUE4Parse.UE4.Assets.Objects;
using CUE4Parse.UE4.Assets.Objects.Properties;
using CUE4Parse.UE4.Objects.Core.Math;

namespace PalDataPack;

public sealed record VerifiedMapSpawnWriteResult(
    string GameBuildId,
    string MappingSha256,
    string ContractReviewId,
    string OutputPath,
    string OutputSha256,
    int PlacementCount,
    int SpawnGroupCount,
    int SpawnEntryCount,
    int ResolvedPlacementCount,
    int UnresolvedPlacementCount,
    int SourceInvertedLevelRangeCount,
    int SourceInvertedCountRangeCount);

public sealed record VerifiedMapSpawnDocument(
    int SchemaVersion,
    string GameBuildId,
    string MappingSha256,
    string ContractReviewId,
    bool Verified,
    string PlacementUnit,
    string GroupUnit,
    int SourceInvertedLevelRangeCount,
    int SourceInvertedCountRangeCount,
    IReadOnlyList<MapSpawnSource> Sources,
    IReadOnlyList<VerifiedMapSpawnPlacement> Placements,
    IReadOnlyList<VerifiedMapSpawnGroup> SpawnGroups);

public sealed record MapSpawnSource(
    string Capability,
    string PackagePath,
    int RowCount);

public sealed record VerifiedMapSpawnPlacement(
    string PlacementId,
    string SourceRowId,
    string? InstanceName,
    string SpawnerName,
    string WorldName,
    string PlacementType,
    string SpawnerType,
    string RadiusType,
    double WorldX,
    double WorldY,
    double WorldZ,
    double StaticRadius,
    double RespawnCooldownSeconds,
    bool HasSpawnGroup);

public sealed record VerifiedMapSpawnGroup(
    string SpawnGroupId,
    string SourceRowId,
    string SpawnerName,
    string SpawnerType,
    int Weight,
    string OnlyTime,
    string OnlyWeather,
    bool HasWorldTreeAura,
    bool AllowsRandomizer,
    IReadOnlyList<VerifiedMapSpawnEntry> Entries);

public sealed record VerifiedMapSpawnEntry(
    int Slot,
    string? PalId,
    string? NpcId,
    int MinimumLevel,
    int MaximumLevel,
    int MinimumCount,
    int MaximumCount,
    bool SourceLevelRangeInverted,
    bool SourceCountRangeInverted);

public static class VerifiedMapSpawnExtractor
{
    private const string PlacementPath =
        "Pal/Content/Pal/DataTable/Spawner/DT_PalSpawnerPlacement";
    private const string SpawnGroupPath =
        "Pal/Content/Pal/DataTable/Spawner/DT_PalWildSpawner";
    private const int MaximumOutputBytes = 32 * 1024 * 1024;

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        WriteIndented = false,
    };

    public static VerifiedMapSpawnWriteResult ExtractToFile(
        SteamInstall install,
        string mappingPath,
        AssetContract contract,
        string outputPath)
    {
        SecureInput.EnsureRegularFile(mappingPath);
        var mappingSha256 = Hashing.Sha256File(mappingPath);
        contract.RequireExtractionIdentity(install.BuildId, mappingSha256);
        RequireExactTables(contract);

        var phase = "mount";
        try
        {
            var provider = SourceContractProbe.OpenProvider(
                install.InstallPath,
                mappingPath);
            phase = "contracted tables";
            var tables = LoadAndValidateTables(provider, contract);
            phase = "spawn groups";
            var groups = ReadSpawnGroups(tables[SpawnGroupPath]);
            var spawnerNames = groups
                .Select(group => group.SpawnerName)
                .ToHashSet(StringComparer.Ordinal);
            phase = "spawn placements";
            var placements = ReadPlacements(tables[PlacementPath], spawnerNames);

            phase = "serialization";
            var sourceInvertedLevelRangeCount = groups
                .SelectMany(group => group.Entries)
                .Count(entry => entry.SourceLevelRangeInverted);
            var sourceInvertedCountRangeCount = groups
                .SelectMany(group => group.Entries)
                .Count(entry => entry.SourceCountRangeInverted);
            var document = new VerifiedMapSpawnDocument(
                SchemaVersion: 1,
                GameBuildId: $"steam:{install.BuildId}",
                MappingSha256: mappingSha256,
                ContractReviewId: contract.ReviewId,
                Verified: true,
                PlacementUnit: "one row in DT_PalSpawnerPlacement",
                GroupUnit: "one weighted row in DT_PalWildSpawner",
                SourceInvertedLevelRangeCount: sourceInvertedLevelRangeCount,
                SourceInvertedCountRangeCount: sourceInvertedCountRangeCount,
                Sources: contract.Tables
                    .OrderBy(table => table.PackagePath, StringComparer.Ordinal)
                    .Select(table => new MapSpawnSource(
                        table.Capability,
                        table.PackagePath,
                        tables[table.PackagePath].RowMap.Count))
                    .ToArray(),
                Placements: placements,
                SpawnGroups: groups);
            var bytes = JsonSerializer.SerializeToUtf8Bytes(document, JsonOptions);
            if (bytes.Length > MaximumOutputBytes)
            {
                throw Failure("verified map-spawn document exceeds its output bound");
            }
            phase = "atomic write";
            var writtenPath = AtomicWrite(outputPath, bytes);
            var resolved = placements.Count(placement => placement.HasSpawnGroup);
            return new VerifiedMapSpawnWriteResult(
                document.GameBuildId,
                mappingSha256,
                contract.ReviewId,
                writtenPath,
                Hashing.Sha256Hex(bytes),
                placements.Count,
                groups.Count,
                groups.Sum(group => group.Entries.Count),
                resolved,
                placements.Count - resolved,
                sourceInvertedLevelRangeCount,
                sourceInvertedCountRangeCount);
        }
        catch (DataPackFailure)
        {
            throw;
        }
        catch (Exception error)
        {
            throw new DataPackFailure(
                DataPackExitCode.MountOrSerialization,
                $"exact-Build map-spawn extraction failed during {phase} "
                + $"({error.GetType().Name})");
        }
    }

    private static Dictionary<string, UDataTable> LoadAndValidateTables(
        DefaultFileProvider provider,
        AssetContract contract)
    {
        var tables = new Dictionary<string, UDataTable>(StringComparer.Ordinal);
        foreach (var tableContract in contract.Tables)
        {
            var table = provider.LoadPackageObject<UDataTable>(tableContract.PackagePath);
            if (tableContract.ExpectedRowCount is not { } expected
                || table.RowMap.Count != expected)
            {
                throw Failure($"exact row count is not pinned: {tableContract.PackagePath}");
            }
            var observed = table.RowMap.Values
                .SelectMany(row => row.Properties)
                .Select(property => property.Name.Text)
                .ToHashSet(StringComparer.Ordinal);
            if (!tableContract.RequiredProperties.All(observed.Contains))
            {
                throw Failure($"contracted table schema drift: {tableContract.PackagePath}");
            }
            tables.Add(tableContract.PackagePath, table);
        }
        return tables;
    }

    private static IReadOnlyList<VerifiedMapSpawnPlacement> ReadPlacements(
        UDataTable table,
        IReadOnlySet<string> spawnerNames)
    {
        var result = new List<VerifiedMapSpawnPlacement>(table.RowMap.Count);
        foreach (var row in table.RowMap.OrderBy(row => row.Key.Text, StringComparer.Ordinal))
        {
            var properties = Properties(row.Value);
            var location = RequiredTag(properties, "Location").GetValue<FVector>();
            if (!double.IsFinite(location.X)
                || !double.IsFinite(location.Y)
                || !double.IsFinite(location.Z))
            {
                throw Failure("spawn placement coordinates are non-finite");
            }
            var rowId = RequiredAscii(row.Key.Text, "placement row");
            var spawnerName = RequiredAscii(Name(properties, "SpawnerName"), "spawner");
            result.Add(new VerifiedMapSpawnPlacement(
                PlacementId: $"spawn-placement:{rowId}",
                SourceRowId: rowId,
                InstanceName: OptionalAsciiName(properties, "InstanceName", "instance"),
                SpawnerName: spawnerName,
                WorldName: RequiredAscii(Name(properties, "WorldName"), "world"),
                PlacementType: StripEnum(Name(properties, "PlacementType")),
                SpawnerType: StripEnum(Name(properties, "SpawnerType")),
                RadiusType: StripEnum(Name(properties, "RadiusType")),
                WorldX: location.X,
                WorldY: location.Y,
                WorldZ: location.Z,
                StaticRadius: Number(properties, "StaticRadius"),
                RespawnCooldownSeconds: Number(properties, "RespawnCoolTime"),
                HasSpawnGroup: spawnerNames.Contains(spawnerName)));
        }
        return result;
    }

    private static IReadOnlyList<VerifiedMapSpawnGroup> ReadSpawnGroups(UDataTable table)
    {
        var result = new List<VerifiedMapSpawnGroup>(table.RowMap.Count);
        foreach (var row in table.RowMap.OrderBy(row => row.Key.Text, StringComparer.Ordinal))
        {
            var properties = Properties(row.Value);
            var rowId = RequiredAscii(row.Key.Text, "spawn group row");
            var entries = Enumerable.Range(1, 3)
                .Select(slot => ReadEntry(properties, rowId, slot))
                .Where(entry => entry is not null)
                .Cast<VerifiedMapSpawnEntry>()
                .ToArray();
            if (entries.Length == 0)
            {
                throw Failure("spawn group contains no Pal or NPC entry");
            }
            result.Add(new VerifiedMapSpawnGroup(
                SpawnGroupId: $"spawn-group:{rowId}",
                SourceRowId: rowId,
                SpawnerName: RequiredAscii(Name(properties, "SpawnerName"), "spawner"),
                SpawnerType: StripEnum(Name(properties, "SpawnerType")),
                Weight: Integer(properties, "Weight"),
                OnlyTime: StripEnum(Name(properties, "OnlyTime")),
                OnlyWeather: StripEnum(Name(properties, "OnlyWeather")),
                HasWorldTreeAura: Boolean(properties, "bHasWorldTreeAura"),
                AllowsRandomizer: Boolean(properties, "bIsAllowRandomizer"),
                Entries: entries));
        }
        return result;
    }

    private static VerifiedMapSpawnEntry? ReadEntry(
        IReadOnlyDictionary<string, FPropertyTagType?> properties,
        string sourceRowId,
        int slot)
    {
        var palId = NullableName(properties, $"Pal_{slot}");
        var npcId = NullableName(properties, $"NPC_{slot}");
        if (palId is null && npcId is null)
        {
            return null;
        }
        var minimumLevel = Integer(properties, $"LvMin_{slot}");
        var maximumLevel = Integer(properties, $"LvMax_{slot}");
        var minimumCount = Integer(properties, $"NumMin_{slot}");
        var maximumCount = Integer(properties, $"NumMax_{slot}");
        if (minimumLevel < 0 || maximumLevel < 0 || minimumCount < 0 || maximumCount < 0)
        {
            throw Failure(
                $"spawn group range is invalid: row={sourceRowId}; slot={slot}; "
                + $"level={minimumLevel}..{maximumLevel}; count={minimumCount}..{maximumCount}");
        }
        var sourceLevelRangeInverted = maximumLevel < minimumLevel;
        var sourceCountRangeInverted = maximumCount < minimumCount;
        return new VerifiedMapSpawnEntry(
            slot,
            palId,
            npcId,
            Math.Min(minimumLevel, maximumLevel),
            Math.Max(minimumLevel, maximumLevel),
            Math.Min(minimumCount, maximumCount),
            Math.Max(minimumCount, maximumCount),
            sourceLevelRangeInverted,
            sourceCountRangeInverted);
    }

    private static Dictionary<string, FPropertyTagType?> Properties(FStructFallback row) =>
        row.Properties.ToDictionary(
            property => property.Name.Text,
            property => property.Tag,
            StringComparer.Ordinal);

    private static FPropertyTagType RequiredTag(
        IReadOnlyDictionary<string, FPropertyTagType?> properties,
        string name) =>
        properties.TryGetValue(name, out var tag) && tag is not null
            ? tag
            : throw Failure($"required map-spawn property is missing: {name}");

    private static string Name(
        IReadOnlyDictionary<string, FPropertyTagType?> properties,
        string name)
    {
        var value = Convert.ToString(
            RequiredTag(properties, name).GenericValue,
            CultureInfo.InvariantCulture);
        return !string.IsNullOrWhiteSpace(value)
            ? value
            : throw Failure($"required map-spawn name is empty: {name}");
    }

    private static string? NullableName(
        IReadOnlyDictionary<string, FPropertyTagType?> properties,
        string name)
    {
        var value = Name(properties, name);
        return string.Equals(value, "None", StringComparison.Ordinal)
            ? null
            : RequiredAscii(value, name);
    }

    private static string? OptionalAsciiName(
        IReadOnlyDictionary<string, FPropertyTagType?> properties,
        string name,
        string field)
    {
        if (!properties.TryGetValue(name, out var tag) || tag is null)
        {
            return null;
        }
        var value = Convert.ToString(tag.GenericValue, CultureInfo.InvariantCulture);
        return string.IsNullOrWhiteSpace(value) ? null : RequiredAscii(value, field);
    }

    private static int Integer(
        IReadOnlyDictionary<string, FPropertyTagType?> properties,
        string name)
    {
        var value = Name(properties, name);
        return int.TryParse(
                value,
                NumberStyles.Integer,
                CultureInfo.InvariantCulture,
                out var parsed)
            ? parsed
            : throw Failure($"required map-spawn integer is invalid: {name}");
    }

    private static double Number(
        IReadOnlyDictionary<string, FPropertyTagType?> properties,
        string name)
    {
        var value = Name(properties, name);
        return double.TryParse(
                value,
                NumberStyles.Float,
                CultureInfo.InvariantCulture,
                out var parsed)
            && double.IsFinite(parsed)
                ? parsed
                : throw Failure($"required map-spawn number is invalid: {name}");
    }

    private static bool Boolean(
        IReadOnlyDictionary<string, FPropertyTagType?> properties,
        string name)
    {
        var value = Name(properties, name);
        return bool.TryParse(value, out var parsed)
            ? parsed
            : throw Failure($"required map-spawn Boolean is invalid: {name}");
    }

    private static string StripEnum(string value)
    {
        var separator = value.LastIndexOf("::", StringComparison.Ordinal);
        return separator >= 0 ? value[(separator + 2)..] : value;
    }

    private static string RequiredAscii(string value, string field)
    {
        if (value.Length is < 1 or > 512
            || value.Any(character => !char.IsAscii(character) || char.IsControl(character)))
        {
            throw Failure($"{field} is not a bounded ASCII identifier");
        }
        return value;
    }

    private static void RequireExactTables(AssetContract contract)
    {
        var expected = new[] { PlacementPath, SpawnGroupPath };
        if (contract.Tables.Count != expected.Length
            || !contract.Tables.Select(table => table.PackagePath)
                .Order(StringComparer.Ordinal)
                .SequenceEqual(expected.Order(StringComparer.Ordinal), StringComparer.Ordinal)
            || contract.Tables.Any(table =>
                !table.Required || table.ExpectedRowCount is null))
        {
            throw Failure("map-spawn extraction requires the closed two-table contract");
        }
    }

    private static string AtomicWrite(string outputPath, ReadOnlySpan<byte> bytes)
    {
        var fullPath = Path.GetFullPath(outputPath);
        var directory = Path.GetDirectoryName(fullPath)
            ?? throw Failure("map-spawn output directory is invalid");
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
