using System.Globalization;
using System.Text.Json;
using System.Text.Json.Serialization;
using CUE4Parse.UE4.Assets.Exports.Engine;
using CUE4Parse.UE4.Assets.Objects;
using CUE4Parse.UE4.Assets.Objects.Properties;
using CUE4Parse.UE4.Objects.Core.i18N;

namespace PalDataPack;

public sealed record VerifiedHumanCatalogWriteResult(
    string GameBuildId,
    string MappingSha256,
    string ContractReviewId,
    string OutputPath,
    string OutputSha256,
    int HumanRowCount,
    int KoreanNameCount,
    int IconMatchCount);

public sealed record VerifiedHumanCatalogSource(
    string Capability,
    string PackagePath,
    int RowCount);

public sealed record VerifiedHumanCatalogRecord(
    string SourceRowId,
    string LocalizationKey,
    string? NameKo,
    string? NameEn,
    bool IsBoss,
    string Organization,
    string Weapon,
    string? IconPackagePath,
    string IconMatchStatus);

public sealed record VerifiedHumanCatalogDocument(
    int SchemaVersion,
    string GameBuildId,
    string MappingSha256,
    string ContractReviewId,
    bool Verified,
    string MatchPolicy,
    IReadOnlyList<VerifiedHumanCatalogSource> Sources,
    IReadOnlyList<VerifiedHumanCatalogRecord> Humans);

public static class VerifiedHumanCatalogExtractor
{
    private const string HumanPath =
        "Pal/Content/Pal/DataTable/Character/DT_PalHumanParameter";
    private const string HumanCommonPath =
        "Pal/Content/Pal/DataTable/Character/DT_PalHumanParameter_Common";
    private const string KoreanNamesPath =
        "Pal/Content/L10N/ko/Pal/DataTable/Text/DT_HumanNameText_Common";
    private const string EnglishNamesPath =
        "Pal/Content/L10N/en/Pal/DataTable/Text/DT_HumanNameText_Common";
    private const string IconTablePath =
        "Pal/Content/Pal/DataTable/Character/DT_PalCharacterIconDataTable";
    private const int MaximumOutputBytes = 4 * 1024 * 1024;

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        WriteIndented = false,
        UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow,
        MaxDepth = 64,
    };

    public static VerifiedHumanCatalogWriteResult ExtractToFile(
        SteamInstall install,
        string mappingPath,
        AssetContract contract,
        string outputPath)
    {
        SecureInput.EnsureRegularFile(mappingPath);
        var mappingSha256 = Hashing.Sha256File(mappingPath);
        contract.RequireExtractionIdentity(install.BuildId, mappingSha256);
        RequireExactContract(contract);

        var phase = "mount";
        try
        {
            var provider = SourceContractProbe.OpenProvider(
                install.InstallPath,
                mappingPath);
            phase = "contracted tables";
            var tables = LoadAndValidateTables(provider, contract);
            ValidateMirroredHumanRows(
                tables[HumanPath],
                tables[HumanCommonPath]);
            phase = "human localization";
            var namesKo = ReadLocalization(tables[KoreanNamesPath]);
            var namesEn = ReadLocalization(tables[EnglishNamesPath]);
            phase = "human icons";
            var icons = ReadIcons(tables[IconTablePath]);
            phase = "human rows";
            var humans = ReadHumans(
                tables[HumanPath],
                namesKo,
                namesEn,
                icons);
            var document = new VerifiedHumanCatalogDocument(
                SchemaVersion: 1,
                GameBuildId: $"steam:{install.BuildId}",
                MappingSha256: mappingSha256,
                ContractReviewId: contract.ReviewId,
                Verified: true,
                MatchPolicy:
                    "exact source-row/localization/icon ID first, then one "
                    + "unique Unreal FName-style case-only match; no prefix "
                    + "stripping or suffix guessing",
                Sources: contract.Tables
                    .OrderBy(table => table.PackagePath, StringComparer.Ordinal)
                    .Select(table => new VerifiedHumanCatalogSource(
                        table.Capability,
                        table.PackagePath,
                        tables[table.PackagePath].RowMap.Count))
                    .ToArray(),
                Humans: humans);
            var bytes = JsonSerializer.SerializeToUtf8Bytes(
                document,
                JsonOptions);
            if (bytes.Length is < 1 or > MaximumOutputBytes)
            {
                throw Failure("verified human catalog exceeds its size bound");
            }
            var writtenPath = AtomicWrite(outputPath, bytes);
            return new VerifiedHumanCatalogWriteResult(
                document.GameBuildId,
                mappingSha256,
                contract.ReviewId,
                writtenPath,
                Hashing.Sha256Hex(bytes),
                humans.Count,
                humans.Count(human => human.NameKo is not null),
                humans.Count(human => human.IconPackagePath is not null));
        }
        catch (DataPackFailure)
        {
            throw;
        }
        catch (Exception error)
        {
            throw new DataPackFailure(
                DataPackExitCode.MountOrSerialization,
                $"exact-Build human extraction failed during {phase} "
                + $"({error.GetType().Name})");
        }
    }

    private static IReadOnlyList<VerifiedHumanCatalogRecord> ReadHumans(
        UDataTable table,
        HumanLookup<string> namesKo,
        HumanLookup<string> namesEn,
        HumanLookup<string> icons)
    {
        var output = new List<VerifiedHumanCatalogRecord>(table.RowMap.Count);
        foreach (var row in table.RowMap.OrderBy(
                     row => row.Key.Text,
                     StringComparer.Ordinal))
        {
            var properties = Properties(row.Value);
            var sourceRowId = RequireIdentifier(
                row.Key.Text,
                "human source row");
            var localizationKey = Name(properties, "OverrideNameTextID");
            var nameKo = UsableDisplayText(
                Resolve(namesKo, localizationKey),
                "ko_Text");
            var nameEn = UsableDisplayText(
                Resolve(namesEn, localizationKey),
                "en_Text");
            var icon = ResolveWithStatus(icons, sourceRowId);
            output.Add(new VerifiedHumanCatalogRecord(
                sourceRowId,
                localizationKey,
                nameKo,
                nameEn,
                Bool(properties, "IsBoss"),
                StripEnum(Name(properties, "Organization")),
                StripEnum(Name(properties, "Weapon")),
                icon.Value,
                icon.Status));
        }
        return output;
    }

    private static void ValidateMirroredHumanRows(
        UDataTable primary,
        UDataTable common)
    {
        var commonRows = common.RowMap.ToDictionary(
            row => row.Key.Text,
            row => Properties(row.Value),
            StringComparer.Ordinal);
        var properties = new[]
        {
            "IsBoss",
            "IsPal",
            "Organization",
            "OverrideNameTextID",
            "Tribe",
            "Weapon",
        };
        foreach (var row in primary.RowMap)
        {
            if (!commonRows.TryGetValue(row.Key.Text, out var mirror))
            {
                throw Failure(
                    $"human common table is missing row: {row.Key.Text}");
            }
            var source = Properties(row.Value);
            if (properties.Any(property => !string.Equals(
                    StringValue(source, property),
                    StringValue(mirror, property),
                    StringComparison.Ordinal)))
            {
                throw Failure(
                    $"human common table row drifted: {row.Key.Text}");
            }
        }
    }

    private static HumanLookup<string> ReadLocalization(UDataTable table)
    {
        var values = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var row in table.RowMap)
        {
            var key = row.Key.Text;
            var text = RequiredTag(Properties(row.Value), "TextData")
                .GetValue<FText>()?.Text?.Trim();
            if (string.IsNullOrWhiteSpace(key)
                || string.IsNullOrWhiteSpace(text)
                || !values.TryAdd(key, text))
            {
                throw Failure($"human localization row is invalid: {key}");
            }
        }
        return BuildLookup(values, "human localization");
    }

    private static HumanLookup<string> ReadIcons(UDataTable table)
    {
        var values = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var row in table.RowMap)
        {
            var key = row.Key.Text;
            var value = StringValue(Properties(row.Value), "Icon");
            if (string.IsNullOrWhiteSpace(key)
                || string.IsNullOrWhiteSpace(value)
                || !values.TryAdd(
                    key,
                    PalIconSourceLinkExtractor.NormalizeIconPath(value)))
            {
                throw Failure($"human icon row is invalid: {key}");
            }
        }
        return BuildLookup(values, "human icon");
    }

    private static HumanLookup<T> BuildLookup<T>(
        IReadOnlyDictionary<string, T> exact,
        string subject)
    {
        var folded = new Dictionary<string, HumanLookupValue<T>>(
            StringComparer.OrdinalIgnoreCase);
        foreach (var (key, value) in exact)
        {
            if (folded.TryGetValue(key, out var previous)
                && !string.Equals(previous.Key, key, StringComparison.Ordinal))
            {
                throw Failure(
                    $"{subject} IDs have a case-only collision: "
                    + $"{previous.Key} / {key}");
            }
            folded[key] = new HumanLookupValue<T>(key, value);
        }
        return new HumanLookup<T>(exact, folded);
    }

    private static T? Resolve<T>(HumanLookup<T> lookup, string key)
        where T : class =>
        ResolveWithStatus(lookup, key).Value;

    private static string? UsableDisplayText(
        string? value,
        string placeholder) =>
        string.IsNullOrWhiteSpace(value)
            || string.Equals(value, placeholder, StringComparison.Ordinal)
            ? null
            : value;

    private static HumanMatch<T> ResolveWithStatus<T>(
        HumanLookup<T> lookup,
        string key)
        where T : class
    {
        if (lookup.Exact.TryGetValue(key, out var exact))
        {
            return new HumanMatch<T>(exact, "exact");
        }
        return lookup.Folded.TryGetValue(key, out var folded)
            ? new HumanMatch<T>(folded.Value, "case_only")
            : new HumanMatch<T>(null, "missing");
    }

    private static Dictionary<string, UDataTable> LoadAndValidateTables(
        CUE4Parse.FileProvider.DefaultFileProvider provider,
        AssetContract contract)
    {
        var tables = new Dictionary<string, UDataTable>(StringComparer.Ordinal);
        foreach (var tableContract in contract.Tables)
        {
            var table = provider.LoadPackageObject<UDataTable>(
                tableContract.PackagePath);
            if (tableContract.ExpectedRowCount is not { } expected
                || table.RowMap.Count != expected)
            {
                throw Failure(
                    $"exact row count is not pinned: "
                    + tableContract.PackagePath);
            }
            var observed = table.RowMap.Values
                .SelectMany(row => row.Properties)
                .Select(property => property.Name.Text)
                .ToHashSet(StringComparer.Ordinal);
            if (!tableContract.RequiredProperties.All(observed.Contains))
            {
                throw Failure(
                    $"contracted table schema drift: "
                    + tableContract.PackagePath);
            }
            tables.Add(tableContract.PackagePath, table);
        }
        return tables;
    }

    private static Dictionary<string, FPropertyTagType?> Properties(
        FStructFallback row) =>
        row.Properties.ToDictionary(
            property => property.Name.Text,
            property => property.Tag,
            StringComparer.Ordinal);

    private static FPropertyTagType RequiredTag(
        IReadOnlyDictionary<string, FPropertyTagType?> properties,
        string name) =>
        properties.TryGetValue(name, out var tag) && tag is not null
            ? tag
            : throw Failure($"required human property is missing: {name}");

    private static string StringValue(
        IReadOnlyDictionary<string, FPropertyTagType?> properties,
        string name) =>
        Convert.ToString(
            RequiredTag(properties, name).GenericValue,
            CultureInfo.InvariantCulture) ?? string.Empty;

    private static string Name(
        IReadOnlyDictionary<string, FPropertyTagType?> properties,
        string name)
    {
        var value = StringValue(properties, name);
        return !string.IsNullOrWhiteSpace(value)
            ? value
            : throw Failure($"required human name property is empty: {name}");
    }

    private static bool Bool(
        IReadOnlyDictionary<string, FPropertyTagType?> properties,
        string name)
    {
        var value = StringValue(properties, name);
        return bool.TryParse(value, out var parsed)
            ? parsed
            : throw Failure($"required human Boolean property is invalid: {name}");
    }

    private static string StripEnum(string value)
    {
        var separator = value.LastIndexOf("::", StringComparison.Ordinal);
        return separator >= 0 ? value[(separator + 2)..] : value;
    }

    private static string RequireIdentifier(string value, string field)
    {
        CatalogContract.RequireIdentifier(value, field);
        return value;
    }

    private static void RequireExactContract(AssetContract contract)
    {
        var expected = new Dictionary<string, int>(StringComparer.Ordinal)
        {
            [HumanPath] = 433,
            [HumanCommonPath] = 433,
            [KoreanNamesPath] = 132,
            [EnglishNamesPath] = 132,
            [IconTablePath] = 674,
        };
        if (contract.Tables.Count != expected.Count
            || contract.Tables.Any(table =>
                !table.Required
                || !expected.TryGetValue(table.PackagePath, out var count)
                || table.ExpectedRowCount != count))
        {
            throw Failure(
                "human extraction requires the closed four-table contract");
        }
    }

    private static string AtomicWrite(
        string outputPath,
        ReadOnlySpan<byte> bytes)
    {
        var fullPath = Path.GetFullPath(outputPath);
        var directory = Path.GetDirectoryName(fullPath)
            ?? throw Failure("human catalog output path has no parent");
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

    private static DataPackFailure Failure(string message) => new(
        DataPackExitCode.ReferenceIntegrity,
        message);

    private sealed record HumanLookup<T>(
        IReadOnlyDictionary<string, T> Exact,
        IReadOnlyDictionary<string, HumanLookupValue<T>> Folded);

    private sealed record HumanLookupValue<T>(string Key, T Value);

    private sealed record HumanMatch<T>(T? Value, string Status);
}
