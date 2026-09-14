using System.Globalization;
using System.Text.Json;
using CUE4Parse.FileProvider;
using CUE4Parse.MappingsProvider.Usmap;
using CUE4Parse.UE4.Assets.Exports.Engine;
using CUE4Parse.UE4.Assets.Objects;
using CUE4Parse.UE4.Assets.Objects.Properties;
using CUE4Parse.UE4.Objects.Core.i18N;
using CUE4Parse.UE4.Versions;

namespace PalDataPack;

public sealed record TableProbeResult(
    string Capability,
    string PackagePath,
    int RowCount,
    IReadOnlyList<string> ObservedProperties);

public sealed record SourceProbeResult(
    string GameBuildId,
    string MappingSha256,
    IReadOnlyList<TableProbeResult> Tables);

public sealed record PathDiscoveryResult(
    string GameBuildId,
    string MappingSha256,
    string Query,
    IReadOnlyList<string> PackagePaths);

public sealed record TableSampleRow(
    string RowKey,
    IReadOnlyDictionary<string, string> Properties);

public sealed record TableSampleResult(
    string GameBuildId,
    string MappingSha256,
    string PackagePath,
    int TotalRowCount,
    IReadOnlyList<TableSampleRow> Rows);

public static class SourceContractProbe
{
    private const int MaximumDiscoveredPaths = 512;

    public static SourceProbeResult Probe(
        SteamInstall install,
        string mappingPath,
        AssetContract contract)
    {
        SecureInput.EnsureRegularFile(mappingPath);
        var mappingSha256 = Hashing.Sha256File(mappingPath);
        contract.RequireProbeIdentity(install.BuildId, mappingSha256);
        try
        {
            var provider = OpenProvider(install.InstallPath, mappingPath);

            var tables = new List<TableProbeResult>(contract.Tables.Count);
            foreach (var tableContract in contract.Tables)
            {
                UDataTable table;
                try
                {
                    table = provider.LoadPackageObject<UDataTable>(
                        tableContract.PackagePath);
                }
                catch when (!tableContract.Required)
                {
                    continue;
                }
                catch (Exception error)
                {
                    throw new DataPackFailure(
                        DataPackExitCode.SourceContractMismatch,
                        $"required table could not be decoded: "
                        + $"{tableContract.PackagePath} ({error.GetType().Name})");
                }
                if (table.RowMap.Count == 0 && tableContract.Required)
                {
                    throw new DataPackFailure(
                        DataPackExitCode.SourceContractMismatch,
                        $"required table is empty: {tableContract.PackagePath}");
                }
                if (tableContract.ExpectedRowCount is { } expectedRowCount
                    && table.RowMap.Count != expectedRowCount)
                {
                    throw new DataPackFailure(
                        DataPackExitCode.SourceContractMismatch,
                        $"table row-count drift: {tableContract.PackagePath}; "
                        + $"expected={expectedRowCount}; actual={table.RowMap.Count}");
                }
                var observed = table.RowMap.Values
                    .SelectMany(row => row.Properties)
                    .Select(property => property.Name.Text)
                    .Distinct(StringComparer.Ordinal)
                    .Order(StringComparer.Ordinal)
                    .ToArray();
                var missing = tableContract.RequiredProperties
                    .Except(observed, StringComparer.Ordinal)
                    .ToArray();
                if (missing.Length > 0)
                {
                    throw new DataPackFailure(
                        DataPackExitCode.SourceContractMismatch,
                        $"table property drift: {tableContract.PackagePath}; "
                        + $"missing=[{string.Join(',', missing)}]; "
                        + $"observed=[{string.Join(',', observed)}]");
                }
                tables.Add(new TableProbeResult(
                    tableContract.Capability,
                    tableContract.PackagePath,
                    table.RowMap.Count,
                    observed));
            }
            return new SourceProbeResult(install.BuildId, mappingSha256, tables);
        }
        catch (DataPackFailure)
        {
            throw;
        }
        catch (Exception)
        {
            throw new DataPackFailure(
                DataPackExitCode.MountOrSerialization,
                "CUE4Parse could not decode the exact-Build source contract");
        }
    }

    public static PathDiscoveryResult DiscoverPaths(
        SteamInstall install,
        string mappingPath,
        AssetContract contract,
        string query)
    {
        if (string.IsNullOrWhiteSpace(query)
            || query.Length > 80
            || query.Any(char.IsControl))
        {
            throw new DataPackFailure(
                DataPackExitCode.Usage,
                "path discovery query is invalid");
        }
        var tokens = query.Split(
            ',',
            StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (tokens.Length is 0 or > 16 || tokens.Any(token => token.Length is 0 or > 40))
        {
            throw new DataPackFailure(
                DataPackExitCode.Usage,
                "path discovery query contains invalid tokens");
        }
        SecureInput.EnsureRegularFile(mappingPath);
        var mappingSha256 = Hashing.Sha256File(mappingPath);
        contract.RequireProbeIdentity(install.BuildId, mappingSha256);
        try
        {
            var provider = OpenProvider(install.InstallPath, mappingPath);
            var paths = provider.Files.Keys
                .Where(path =>
                    path.StartsWith("Pal/Content/", StringComparison.OrdinalIgnoreCase)
                    && tokens.Any(token =>
                        path.Contains(token, StringComparison.OrdinalIgnoreCase))
                    && path.EndsWith(".uasset", StringComparison.OrdinalIgnoreCase))
                .Select(path => path[..^".uasset".Length])
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .Order(StringComparer.OrdinalIgnoreCase)
                .Take(MaximumDiscoveredPaths)
                .ToArray();
            return new PathDiscoveryResult(
                install.BuildId,
                mappingSha256,
                query,
                paths);
        }
        catch (DataPackFailure)
        {
            throw;
        }
        catch (Exception)
        {
            throw new DataPackFailure(
                DataPackExitCode.MountOrSerialization,
                "CUE4Parse could not inventory exact-Build package paths");
        }
    }

    public static TableSampleResult SampleTable(
        SteamInstall install,
        string mappingPath,
        AssetContract contract,
        string packagePath,
        int limit,
        string? rowKey = null,
        string? rowContains = null,
        string? textContains = null)
    {
        var textTokens = textContains?.Split(
            ',',
            StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (limit is < 1 or > 20
            || rowKey is { Length: > 128 }
            || rowKey?.Any(char.IsControl) == true
            || rowContains is { Length: > 128 }
            || rowContains?.Any(char.IsControl) == true
            || textContains is { Length: > 128 }
            || textContains?.Any(char.IsControl) == true
            || textTokens is { Length: 0 or > 32 }
            || textTokens?.Any(token => token.Length is 0 or > 40) == true
            || new[] { rowKey, rowContains, textContains }.Count(value => value is not null) > 1
            || !contract.Tables.Any(table =>
                string.Equals(table.PackagePath, packagePath, StringComparison.Ordinal)))
        {
            throw new DataPackFailure(
                DataPackExitCode.Usage,
                "table sampling is limited to 1..20 rows from a contracted package");
        }
        SecureInput.EnsureRegularFile(mappingPath);
        var mappingSha256 = Hashing.Sha256File(mappingPath);
        contract.RequireProbeIdentity(install.BuildId, mappingSha256);
        try
        {
            var provider = OpenProvider(install.InstallPath, mappingPath);
            var table = provider.LoadPackageObject<UDataTable>(packagePath);
            var rows = table.RowMap
                .Where(row => rowKey is null
                    || string.Equals(row.Key.Text, rowKey, StringComparison.Ordinal))
                .Where(row => rowContains is null
                    || row.Key.Text.Contains(rowContains, StringComparison.OrdinalIgnoreCase))
                .Where(row => textTokens is null
                    || row.Value.Properties.Any(property =>
                        property.Name.Text == "TextData"
                        && NormalizeTag(property.Tag, depth: 0) is string text
                        && textTokens.Any(token =>
                            text.Contains(token, StringComparison.OrdinalIgnoreCase))))
                .OrderBy(row => row.Key.Text, StringComparer.Ordinal)
                .Take(limit)
                .Select(row => new TableSampleRow(
                    row.Key.Text,
                    row.Value.Properties
                        .OrderBy(property => property.Name.Text, StringComparer.Ordinal)
                        .ToDictionary(
                            property => property.Name.Text,
                            property => BoundedValue(property.Tag),
                            StringComparer.Ordinal)))
                .ToArray();
            return new TableSampleResult(
                install.BuildId,
                mappingSha256,
                packagePath,
                table.RowMap.Count,
                rows);
        }
        catch (DataPackFailure)
        {
            throw;
        }
        catch (Exception)
        {
            throw new DataPackFailure(
                DataPackExitCode.MountOrSerialization,
                "CUE4Parse could not sample the contracted table");
        }
    }

    private static string BoundedValue(FPropertyTagType? tag)
    {
        var value = NormalizeTag(tag, depth: 0);
        var text = value is string scalar
            ? scalar
            : JsonSerializer.Serialize(value);
        text = new string(text
            .Where(character => !char.IsControl(character) || character is '\t')
            .Take(16 * 1024)
            .ToArray());
        return text;
    }

    private static object? NormalizeTag(FPropertyTagType? tag, int depth)
    {
        if (tag is null)
        {
            return null;
        }
        if (depth >= 8)
        {
            return "<maximum-depth>";
        }
        if (tag is ArrayProperty { Value: { } array })
        {
            return array.Properties
                .Take(512)
                .Select(element => NormalizeTag(element, depth + 1))
                .ToArray();
        }
        if (tag is StructProperty { Value.StructType: FStructFallback fallback })
        {
            return fallback.Properties
                .OrderBy(property => property.Name.Text, StringComparer.Ordinal)
                .ToDictionary(
                    property => property.ArrayIndex > 0
                        ? $"{property.Name.Text}[{property.ArrayIndex}]"
                        : property.Name.Text,
                    property => NormalizeTag(property.Tag, depth + 1),
                    StringComparer.Ordinal);
        }
        if (tag.GenericValue is FText text)
        {
            return text.Text;
        }
        return Convert.ToString(tag.GenericValue, CultureInfo.InvariantCulture)
            ?? string.Empty;
    }

    internal static DefaultFileProvider OpenProvider(
        string installPath,
        string mappingPath)
    {
        var provider = new DefaultFileProvider(
            installPath,
            SearchOption.AllDirectories,
            new VersionContainer(EGame.GAME_UE5_1),
            StringComparer.OrdinalIgnoreCase)
        {
            MappingsContainer = new FileUsmapTypeMappingsProvider(mappingPath),
        };
        provider.Initialize();
        if (provider.Mount() <= 0)
        {
            throw new DataPackFailure(
                DataPackExitCode.MountOrSerialization,
                "CUE4Parse mounted no game containers");
        }
        provider.LoadVirtualPaths();
        return provider;
    }
}
