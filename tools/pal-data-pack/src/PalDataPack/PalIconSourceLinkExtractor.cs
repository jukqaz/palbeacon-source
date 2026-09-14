using System.Globalization;
using System.Text.Json;
using System.Text.Json.Serialization;
using CUE4Parse.UE4.Assets.Exports.Engine;
using CUE4Parse.UE4.Assets.Objects;

namespace PalDataPack;

public sealed record PalIconTableEntry(
    string RowKey,
    string PackagePath);

public sealed record PalNameIconSourceLink(
    string LocalizationKey,
    IReadOnlyList<string> ParameterRowKeys,
    IReadOnlyList<string> Tribes,
    IReadOnlyList<string> IconTableRowKeys,
    IReadOnlyList<string> PackagePaths,
    string Resolution);

public sealed record PalParameterIconSourceLink(
    string RowKey,
    string LocalizationKey,
    string Tribe,
    IReadOnlyList<string> IconTableRowKeys,
    IReadOnlyList<string> PackagePaths,
    string Resolution);

public sealed record PalIconSourceLinkReport(
    int SchemaMajor,
    string GameBuildId,
    string MappingSha256,
    string ReviewId,
    bool Verified,
    string SourceKind,
    string ParameterTablePackagePath,
    int ParameterTableRowCount,
    string IconTablePackagePath,
    int IconTableRowCount,
    IReadOnlyList<PalIconTableEntry> IconTableEntries,
    IReadOnlyList<PalParameterIconSourceLink> ParameterRowLinks,
    IReadOnlyList<PalNameIconSourceLink> LocalizedNameLinks);

public sealed record PalIconSourceLinkExtractionResult(
    string OutputPath,
    int IconTableEntryCount,
    int LocalizedNameLinkCount,
    int ResolvedLocalizedNameLinkCount);

public static class PalIconSourceLinkExtractor
{
    internal const string ParameterTablePath =
        "Pal/Content/Pal/DataTable/Character/DT_PalMonsterParameter";
    internal const string IconTablePath =
        "Pal/Content/Pal/DataTable/Character/DT_PalCharacterIconDataTable";

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        WriteIndented = true,
        UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow,
        MaxDepth = 64,
    };

    public static PalIconSourceLinkExtractionResult ExtractToFile(
        SteamInstall install,
        string mappingPath,
        AssetContract contract,
        string outputPath)
    {
        SecureInput.EnsureRegularFile(mappingPath);
        var mappingSha256 = Hashing.Sha256File(mappingPath);
        contract.RequireExtractionIdentity(install.BuildId, mappingSha256);
        RequireExactContract(contract);

        try
        {
            var provider = SourceContractProbe.OpenProvider(
                install.InstallPath,
                mappingPath);
            var parameterTable =
                provider.LoadPackageObject<UDataTable>(ParameterTablePath);
            var iconTable = provider.LoadPackageObject<UDataTable>(IconTablePath);
            if (parameterTable.RowMap.Count != 753
                || iconTable.RowMap.Count != 674)
            {
                throw Failure("Pal icon source-link table row count drifted");
            }
            RequireProperties(
                parameterTable,
                ["IsPal", "OverrideNameTextID", "Tribe"]);
            RequireProperties(iconTable, ["Icon"]);
            var iconEntries = iconTable.RowMap
                .Select(row => new PalIconTableEntry(
                    row.Key.Text,
                    NormalizeIconPath(Scalar(row.Value, "Icon"))))
                .OrderBy(entry => entry.RowKey, StringComparer.Ordinal)
                .ToArray();
            var iconEntriesInsensitive = iconEntries
                .GroupBy(entry => entry.RowKey, StringComparer.OrdinalIgnoreCase)
                .ToDictionary(
                    group => group.Key,
                    group => group.ToArray(),
                    StringComparer.OrdinalIgnoreCase);

            var parameterRows = parameterTable.RowMap
                .Select(row => new
                {
                    RowKey = row.Key.Text,
                    IsPal = Scalar(row.Value, "IsPal"),
                    LocalizationKey = Scalar(row.Value, "OverrideNameTextID"),
                    Tribe = EnumValue(Scalar(row.Value, "Tribe")),
                })
                .Where(row => string.Equals(
                    row.IsPal,
                    bool.TrueString,
                    StringComparison.OrdinalIgnoreCase))
                .OrderBy(row => row.RowKey, StringComparer.Ordinal)
                .ToArray();
            var parameterLinks = parameterRows
                .Select(row =>
                {
                    var resolvedEntries = ResolveIconEntries(
                        row.Tribe,
                        iconEntriesInsensitive);
                    var paths = resolvedEntries
                        .Select(entry => entry.PackagePath)
                        .Distinct(StringComparer.Ordinal)
                        .Order(StringComparer.Ordinal)
                        .ToArray();
                    return new PalParameterIconSourceLink(
                        row.RowKey,
                        row.LocalizationKey,
                        row.Tribe,
                        resolvedEntries
                            .Select(entry => entry.RowKey)
                            .ToArray(),
                        paths,
                        Resolution(paths));
                })
                .ToArray();
            var nameGroups = parameterRows
                .Where(row => IsUsable(row.LocalizationKey)
                    && IsUsable(row.Tribe))
                .GroupBy(row => row.LocalizationKey, StringComparer.Ordinal)
                .OrderBy(group => group.Key, StringComparer.Ordinal)
                .Select(group =>
                {
                    var tribes = group
                        .Select(row => row.Tribe)
                        .Distinct(StringComparer.Ordinal)
                        .Order(StringComparer.Ordinal)
                        .ToArray();
                    var resolvedEntries = tribes
                        .SelectMany(tribe => ResolveIconEntries(
                            tribe,
                            iconEntriesInsensitive))
                        .DistinctBy(entry => entry.RowKey, StringComparer.Ordinal)
                        .OrderBy(entry => entry.RowKey, StringComparer.Ordinal)
                        .ToArray();
                    var paths = resolvedEntries
                        .Select(entry => entry.PackagePath)
                        .Distinct(StringComparer.Ordinal)
                        .Order(StringComparer.Ordinal)
                        .ToArray();
                    return new PalNameIconSourceLink(
                        group.Key,
                        group
                            .Select(row => row.RowKey)
                            .Distinct(StringComparer.Ordinal)
                            .Order(StringComparer.Ordinal)
                            .ToArray(),
                        tribes,
                        resolvedEntries
                            .Select(entry => entry.RowKey)
                            .ToArray(),
                        paths,
                        Resolution(paths));
                })
                .ToArray();

            var report = new PalIconSourceLinkReport(
                CatalogContract.SchemaMajor,
                install.BuildId,
                mappingSha256,
                contract.ReviewId,
                Verified: true,
                SourceKind: "installed_game_data_table",
                ParameterTablePath,
                parameterTable.RowMap.Count,
                IconTablePath,
                iconTable.RowMap.Count,
                iconEntries,
                parameterLinks,
                nameGroups);
            WriteNewFile(outputPath, report);
            return new PalIconSourceLinkExtractionResult(
                Path.GetFullPath(outputPath),
                iconEntries.Length,
                nameGroups.Length,
                nameGroups.Count(link =>
                    string.Equals(
                        link.Resolution,
                        "resolved",
                        StringComparison.Ordinal)));
        }
        catch (DataPackFailure)
        {
            throw;
        }
        catch (Exception error)
        {
            throw Failure(
                "CUE4Parse could not extract the reviewed Pal icon source links "
                + $"({error.GetType().Name}: {error.Message})");
        }
    }

    private static void RequireExactContract(AssetContract contract)
    {
        var expected = new Dictionary<
            string,
            (int RowCount, string[] Properties)>(StringComparer.Ordinal)
        {
            [ParameterTablePath] = (
                753,
                ["IsPal", "OverrideNameTextID", "Tribe"]),
            [IconTablePath] = (674, ["Icon"]),
        };
        if (contract.Tables.Count != expected.Count
            || contract.Tables.Any(table =>
                !table.Required
                || !expected.TryGetValue(table.PackagePath, out var definition)
                || table.ExpectedRowCount != definition.RowCount
                || !table.RequiredProperties
                    .Order(StringComparer.Ordinal)
                    .SequenceEqual(
                        definition.Properties.Order(StringComparer.Ordinal),
                        StringComparer.Ordinal)))
        {
            throw Failure("Pal icon source-link table contract drifted");
        }
    }

    private static void RequireProperties(
        UDataTable table,
        IReadOnlyList<string> requiredProperties)
    {
        var observed = table.RowMap.Values
            .SelectMany(row => row.Properties)
            .Select(property => property.Name.Text)
            .Distinct(StringComparer.Ordinal)
            .ToHashSet(StringComparer.Ordinal);
        var missing = requiredProperties
            .Where(property => !observed.Contains(property))
            .ToArray();
        if (missing.Length > 0)
        {
            throw Failure(
                $"Pal icon source-link table properties drifted: "
                + $"{string.Join(',', missing)}");
        }
    }

    private static string Scalar(FStructFallback row, string propertyName)
    {
        var property = row.Properties.SingleOrDefault(property =>
            string.Equals(
                property.Name.Text,
                propertyName,
                StringComparison.Ordinal));
        return Convert.ToString(
                property?.Tag?.GenericValue,
                CultureInfo.InvariantCulture)
            ?? string.Empty;
    }

    private static string EnumValue(string value)
    {
        var separator = value.LastIndexOf("::", StringComparison.Ordinal);
        return separator >= 0 ? value[(separator + 2)..] : value;
    }

    public static string NormalizeIconPath(string value)
    {
        if (!value.StartsWith("/Game/", StringComparison.Ordinal)
            || value.Any(char.IsControl))
        {
            throw Failure($"Pal icon table contains an invalid asset path: {value}");
        }
        var objectSeparator = value.LastIndexOf('.');
        var objectPath = objectSeparator > value.LastIndexOf('/')
            ? value[..objectSeparator]
            : value;
        return "Pal/Content/" + objectPath["/Game/".Length..];
    }

    private static bool IsUsable(string value) =>
        !string.IsNullOrWhiteSpace(value)
        && !string.Equals(value, "None", StringComparison.Ordinal);

    private static PalIconTableEntry[] ResolveIconEntries(
        string tribe,
        IReadOnlyDictionary<string, PalIconTableEntry[]> iconEntries)
    {
        if (!IsUsable(tribe)
            || !iconEntries.TryGetValue(tribe, out var entries))
        {
            return [];
        }
        return entries
            .OrderBy(entry => entry.RowKey, StringComparer.Ordinal)
            .ToArray();
    }

    private static string Resolution(IReadOnlyList<string> paths) =>
        paths.Count switch
        {
            0 => "missing_icon_table_row",
            1 => "resolved",
            _ => "ambiguous_multiple_icons",
        };

    private static void WriteNewFile(
        string outputPath,
        PalIconSourceLinkReport report)
    {
        var fullOutput = Path.GetFullPath(outputPath);
        if (File.Exists(fullOutput) || Directory.Exists(fullOutput))
        {
            throw Failure("Pal icon source-link output must not exist");
        }
        var parent = Path.GetDirectoryName(fullOutput)
            ?? throw Failure("Pal icon source-link output parent is invalid");
        Directory.CreateDirectory(parent);
        SecureInput.EnsurePlainDirectory(parent);
        var staging = Path.Combine(
            parent,
            $".{Path.GetFileName(fullOutput)}.{Guid.NewGuid():N}.staging");
        try
        {
            File.WriteAllBytes(
                staging,
                JsonSerializer.SerializeToUtf8Bytes(report, JsonOptions));
            using (var stream = new FileStream(
                       staging,
                       FileMode.Open,
                       FileAccess.Read,
                       FileShare.Read,
                       64 * 1024,
                       FileOptions.SequentialScan))
            {
                _ = JsonSerializer.Deserialize<PalIconSourceLinkReport>(
                        stream,
                        JsonOptions)
                    ?? throw Failure(
                        "Pal icon source-link report could not be reopened");
            }
            File.Move(staging, fullOutput);
        }
        finally
        {
            if (File.Exists(staging))
            {
                File.Delete(staging);
            }
        }
    }

    private static DataPackFailure Failure(string message) =>
        new(DataPackExitCode.SourceContractMismatch, message);
}
