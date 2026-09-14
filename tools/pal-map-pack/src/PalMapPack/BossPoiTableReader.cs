using System.Buffers.Binary;
using System.Security.Cryptography;
using System.Text;

namespace PalMapPack;

/// <summary>
/// Transport-neutral values accepted from an exact-Build boss-location table adapter.
/// The CUE4Parse adapter deliberately lives outside this normalization boundary.
/// </summary>
public abstract record BossPoiSourceValue;

public sealed record BossPoiNameValue(string Text) : BossPoiSourceValue;

public sealed record BossPoiVectorValue(double X, double Y, double Z) : BossPoiSourceValue;

public sealed record BossPoiIntegerValue(int Value) : BossPoiSourceValue;

public sealed record BossPoiSourceRow(
    string RawRowKey,
    IReadOnlyDictionary<string, BossPoiSourceValue> Properties);

public enum BossPoiKind
{
    PalFieldBoss,
    HumanFieldBoss,
    BossRegion,
}

public enum BossPoiNameTable
{
    Pal,
    Human,
    WorldMap,
}

public sealed record BossPoiNameLookup(BossPoiNameTable Table, string Key);

public interface IBossPoiLocalizedNameResolver
{
    string? Resolve(BossPoiNameTable table, string key);
}

public sealed record NormalizedBossPoi(
    string LogicalId,
    BossPoiKind Kind,
    string DisplayName,
    BossPoiNameLookup NameLookup,
    string SpawnerId,
    string CharacterId,
    double WorldX,
    double WorldY,
    double WorldZ,
    int Level,
    IReadOnlyList<string> RawRowKeys);

public sealed record BossPoiTableResult(
    IReadOnlyList<NormalizedBossPoi> Pois,
    int RawRowCount,
    int DuplicateRowCount);

/// <summary>
/// Fail-closed normalization for DT_BossSpawnerLoactionData rows observed in Build 24181527.
/// </summary>
public static class BossPoiTableReader
{
    private static readonly string[] ExactPropertyNames =
    [
        "CharacterID",
        "Level",
        "Location",
        "SpawnerID",
    ];

    public static BossPoiTableResult Normalize(
        IEnumerable<BossPoiSourceRow> sourceRows,
        IBossPoiLocalizedNameResolver localizedNames)
    {
        ArgumentNullException.ThrowIfNull(sourceRows);
        ArgumentNullException.ThrowIfNull(localizedNames);

        var rawRows = sourceRows.ToArray();
        if (rawRows.Length == 0)
        {
            throw Failure("boss POI table is empty");
        }

        var seenRawRowKeys = new HashSet<string>(StringComparer.Ordinal);
        var decoded = new List<DecodedRow>(rawRows.Length);
        foreach (var sourceRow in rawRows)
        {
            if (sourceRow is null
                || string.IsNullOrWhiteSpace(sourceRow.RawRowKey)
                || !seenRawRowKeys.Add(sourceRow.RawRowKey))
            {
                throw Failure("boss POI raw row key is missing or duplicated");
            }

            decoded.Add(Decode(sourceRow));
        }

        var normalized = new List<NormalizedBossPoi>();
        foreach (var group in decoded.GroupBy(row => row.SemanticKey))
        {
            var row = group.First();
            var classification = Classify(row.SpawnerId, row.CharacterId);
            var displayName = localizedNames.Resolve(
                classification.NameLookup.Table,
                classification.NameLookup.Key);
            if (string.IsNullOrWhiteSpace(displayName))
            {
                throw Failure("boss POI localized display name did not resolve");
            }

            normalized.Add(new NormalizedBossPoi(
                CreateLogicalId(row, classification),
                classification.Kind,
                displayName,
                classification.NameLookup,
                row.SpawnerId,
                row.CharacterId,
                row.WorldX,
                row.WorldY,
                row.WorldZ,
                row.Level,
                group.Select(item => item.RawRowKey)
                    .OrderBy(key => key, StringComparer.Ordinal)
                    .ToArray()));
        }

        var logicalIdCollision = normalized
            .GroupBy(poi => poi.LogicalId, StringComparer.Ordinal)
            .FirstOrDefault(group => group.Count() != 1);
        if (logicalIdCollision is not null)
        {
            throw Failure("non-identical boss POI rows produced one logical identity");
        }

        var ordered = normalized
            .OrderBy(poi => poi.LogicalId, StringComparer.Ordinal)
            .ToArray();
        return new BossPoiTableResult(
            ordered,
            rawRows.Length,
            rawRows.Length - ordered.Length);
    }

    private static DecodedRow Decode(BossPoiSourceRow row)
    {
        if (row.Properties is null
            || row.Properties.Count != ExactPropertyNames.Length
            || !row.Properties.Keys
                .OrderBy(key => key, StringComparer.Ordinal)
                .SequenceEqual(ExactPropertyNames, StringComparer.Ordinal)
            || row.Properties["SpawnerID"] is not BossPoiNameValue spawner
            || row.Properties["CharacterID"] is not BossPoiNameValue character
            || row.Properties["Location"] is not BossPoiVectorValue location
            || row.Properties["Level"] is not BossPoiIntegerValue level)
        {
            throw Failure("boss POI row does not match the exact four-property schema");
        }
        if (string.IsNullOrWhiteSpace(spawner.Text)
            || string.IsNullOrWhiteSpace(character.Text)
            || !double.IsFinite(location.X)
            || !double.IsFinite(location.Y)
            || !double.IsFinite(location.Z)
            || level.Value <= 0)
        {
            throw Failure("boss POI row contains an invalid required value");
        }

        return new DecodedRow(
            row.RawRowKey,
            spawner.Text,
            character.Text,
            location.X,
            location.Y,
            location.Z,
            level.Value,
            new SemanticKey(
                spawner.Text,
                character.Text,
                BitConverter.DoubleToInt64Bits(location.X),
                BitConverter.DoubleToInt64Bits(location.Y),
                BitConverter.DoubleToInt64Bits(location.Z),
                level.Value));
    }

    private static Classification Classify(string spawnerId, string characterId)
    {
        if (!string.Equals(characterId, "None", StringComparison.Ordinal))
        {
            const string bossPrefix = "BOSS_";
            if (!characterId.StartsWith(bossPrefix, StringComparison.OrdinalIgnoreCase)
                || characterId.Length == bossPrefix.Length)
            {
                throw Failure("pal boss CharacterID does not use the contracted BOSS_ family");
            }
            var speciesId = characterId[bossPrefix.Length..];
            return new Classification(
                BossPoiKind.PalFieldBoss,
                new BossPoiNameLookup(BossPoiNameTable.Pal, "PAL_NAME_" + speciesId),
                bossPrefix + speciesId);
        }
        if (spawnerId.StartsWith("BOSS_", StringComparison.Ordinal)
            && spawnerId.Length > "BOSS_".Length)
        {
            return new Classification(
                BossPoiKind.HumanFieldBoss,
                new BossPoiNameLookup(BossPoiNameTable.Human, "NAME_" + spawnerId),
                characterId);
        }
        if (spawnerId.StartsWith("REGION_", StringComparison.Ordinal)
            && spawnerId.Length > "REGION_".Length)
        {
            return new Classification(
                BossPoiKind.BossRegion,
                new BossPoiNameLookup(BossPoiNameTable.WorldMap, spawnerId),
                characterId);
        }
        throw Failure("boss POI row belongs to an unknown CharacterID/SpawnerID family");
    }

    private static string CreateLogicalId(DecodedRow row, Classification classification)
    {
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        hash.AppendData("pal-boss-poi-v1\0"u8);
        AppendByte(hash, (byte)classification.Kind);
        AppendString(hash, row.SpawnerId);
        AppendString(hash, classification.CanonicalCharacterId);
        AppendDouble(hash, row.WorldX);
        AppendDouble(hash, row.WorldY);
        AppendDouble(hash, row.WorldZ);
        return "boss:" + Convert.ToHexStringLower(hash.GetHashAndReset());
    }

    private static void AppendByte(IncrementalHash hash, byte value) => hash.AppendData([value]);

    private static void AppendString(IncrementalHash hash, string value)
    {
        var bytes = Encoding.UTF8.GetBytes(value);
        Span<byte> length = stackalloc byte[sizeof(uint)];
        BinaryPrimitives.WriteUInt32LittleEndian(length, checked((uint)bytes.Length));
        hash.AppendData(length);
        hash.AppendData(bytes);
    }

    private static void AppendDouble(IncrementalHash hash, double value)
    {
        if (value == 0)
        {
            value = 0;
        }
        Span<byte> bytes = stackalloc byte[sizeof(ulong)];
        BinaryPrimitives.WriteUInt64LittleEndian(bytes, BitConverter.DoubleToUInt64Bits(value));
        hash.AppendData(bytes);
    }

    private static MapPackFailure Failure(string message) =>
        new(ExitCodes.AssetContractMismatch, message);

    private sealed record DecodedRow(
        string RawRowKey,
        string SpawnerId,
        string CharacterId,
        double WorldX,
        double WorldY,
        double WorldZ,
        int Level,
        SemanticKey SemanticKey);

    private sealed record SemanticKey(
        string SpawnerId,
        string CharacterId,
        long WorldXBits,
        long WorldYBits,
        long WorldZBits,
        int Level);

    private sealed record Classification(
        BossPoiKind Kind,
        BossPoiNameLookup NameLookup,
        string CanonicalCharacterId);
}
