using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;
using CUE4Parse.UE4.Assets.Exports.Engine;
using CUE4Parse.UE4.Objects.Core.i18N;

namespace PalDataPack;

public sealed record VerifiedKoreanCatalogWriteResult(
    string GameBuildId,
    string MappingSha256,
    string ContractReviewId,
    string OutputDirectory,
    int PalCount,
    int ActiveSkillCount,
    int PassiveSkillCount,
    IReadOnlyDictionary<string, string> FileSha256);

public sealed record KoreanCatalogText(
    string LocalizedName,
    string? Description);

public sealed record VerifiedKoreanCatalogManifest(
    int SchemaVersion,
    string GameBuildId,
    string MappingSha256,
    string ContractReviewId,
    bool Verified,
    string Language,
    string LocalizationOrigin,
    bool MachineTranslationAllowed,
    IReadOnlyList<KoreanCatalogSource> Sources,
    IReadOnlyDictionary<string, KoreanCatalogFile> Files);

public sealed record KoreanCatalogSource(
    string PackagePath,
    int RowCount);

public sealed record KoreanCatalogFile(
    string RelativePath,
    int RecordCount,
    string Sha256);

public static partial class VerifiedKoreanCatalogExtractor
{
    private const string PalNamesPath =
        "Pal/Content/L10N/ko/Pal/DataTable/Text/DT_PalNameText_Common";
    private const string PalDescriptionsPath =
        "Pal/Content/L10N/ko/Pal/DataTable/Text/DT_PalLongDescriptionText";
    private const string SkillNamesPath =
        "Pal/Content/L10N/ko/Pal/DataTable/Text/DT_SkillNameText_Common";
    private const string SkillDescriptionsPath =
        "Pal/Content/L10N/ko/Pal/DataTable/Text/DT_SkillDescText_Common";
    private const int MaximumOutputBytes = 8 * 1024 * 1024;

    private static readonly JsonSerializerOptions CatalogJsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        WriteIndented = true,
    };

    [GeneratedRegex(
        @"<characterName\s+id=\|([^|]+)\|/>",
        RegexOptions.CultureInvariant)]
    private static partial Regex CharacterNameMarkup();

    public static VerifiedKoreanCatalogWriteResult ExtractToDirectory(
        SteamInstall install,
        string mappingPath,
        AssetContract contract,
        string outputDirectory)
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
            var tables = contract.Tables.ToDictionary(
                table => table.PackagePath,
                table => provider.LoadPackageObject<UDataTable>(table.PackagePath),
                StringComparer.Ordinal);
            foreach (var tableContract in contract.Tables)
            {
                var table = tables[tableContract.PackagePath];
                if (table.RowMap.Count != tableContract.ExpectedRowCount)
                {
                    throw Failure(
                        $"Korean source table row count drifted: {tableContract.PackagePath}");
                }
            }

            var palNames = ReadTextRows(tables[PalNamesPath], "PAL_NAME_");
            var palDescriptions = ReadTextRows(
                tables[PalDescriptionsPath],
                "PAL_LONG_DESC_");
            var skillNames = ReadAllTextRows(tables[SkillNamesPath]);
            var skillDescriptions = ReadAllTextRows(tables[SkillDescriptionsPath]);

            var pals = BuildPals(palNames, palDescriptions);
            var activeSkills = BuildActiveSkills(skillNames, skillDescriptions);
            var passiveSkills = BuildPassiveSkills(skillNames, skillDescriptions);

            var files = new SortedDictionary<string, (byte[] Bytes, int Count)>(
                StringComparer.Ordinal)
            {
                ["pals"] = (
                    JsonSerializer.SerializeToUtf8Bytes(pals, CatalogJsonOptions),
                    pals.Count),
                ["active_skills"] = (
                    JsonSerializer.SerializeToUtf8Bytes(activeSkills, CatalogJsonOptions),
                    activeSkills.Count),
                ["passive_skills"] = (
                    JsonSerializer.SerializeToUtf8Bytes(passiveSkills, CatalogJsonOptions),
                    passiveSkills.Count),
            };
            if (files.Values.Any(file => file.Bytes.Length > MaximumOutputBytes))
            {
                throw Failure("verified Korean catalog output exceeds its bound");
            }

            var outputRoot = Path.GetFullPath(outputDirectory);
            Directory.CreateDirectory(outputRoot);
            var manifestFiles = new SortedDictionary<string, KoreanCatalogFile>(
                StringComparer.Ordinal);
            foreach (var (id, file) in files)
            {
                var relativePath = $"{id}.json";
                AtomicWrite(Path.Combine(outputRoot, relativePath), file.Bytes);
                manifestFiles.Add(
                    id,
                    new KoreanCatalogFile(
                        relativePath,
                        file.Count,
                        Hashing.Sha256Hex(file.Bytes)));
            }

            var manifest = new VerifiedKoreanCatalogManifest(
                SchemaVersion: 1,
                GameBuildId: $"steam:{install.BuildId}",
                MappingSha256: mappingSha256,
                ContractReviewId: contract.ReviewId,
                Verified: true,
                Language: "ko",
                LocalizationOrigin: "game_l10n",
                MachineTranslationAllowed: false,
                Sources: contract.Tables
                    .OrderBy(table => table.PackagePath, StringComparer.Ordinal)
                    .Select(table => new KoreanCatalogSource(
                        table.PackagePath,
                        tables[table.PackagePath].RowMap.Count))
                    .ToArray(),
                Files: manifestFiles);
            AtomicWrite(
                Path.Combine(outputRoot, "manifest.v1.json"),
                JsonSerializer.SerializeToUtf8Bytes(manifest, CatalogJsonOptions));

            return new VerifiedKoreanCatalogWriteResult(
                manifest.GameBuildId,
                mappingSha256,
                contract.ReviewId,
                outputRoot,
                pals.Count,
                activeSkills.Count,
                passiveSkills.Count,
                manifestFiles.ToDictionary(
                    pair => pair.Key,
                    pair => pair.Value.Sha256,
                    StringComparer.Ordinal));
        }
        catch (DataPackFailure)
        {
            throw;
        }
        catch (Exception error)
        {
            throw new DataPackFailure(
                DataPackExitCode.MountOrSerialization,
                $"exact-Build Korean catalog extraction failed ({error.GetType().Name})");
        }
    }

    private static SortedDictionary<string, KoreanCatalogText> BuildPals(
        IReadOnlyDictionary<string, string> names,
        IReadOnlyDictionary<string, string> descriptions)
    {
        var output = new SortedDictionary<string, KoreanCatalogText>(StringComparer.Ordinal);
        foreach (var (id, name) in names)
        {
            descriptions.TryGetValue(id, out var description);
            output.Add(
                id,
                new KoreanCatalogText(
                    name,
                    ResolveCharacterNames(description, names)));
        }
        return output;
    }

    private static SortedDictionary<string, KoreanCatalogText> BuildActiveSkills(
        IReadOnlyDictionary<string, string> names,
        IReadOnlyDictionary<string, string> descriptions)
    {
        const string prefix = "ACTION_SKILL_";
        var output = new SortedDictionary<string, KoreanCatalogText>(StringComparer.Ordinal);
        foreach (var (rowKey, name) in names.Where(
                     pair => pair.Key.StartsWith(prefix, StringComparison.Ordinal)))
        {
            descriptions.TryGetValue(rowKey, out var description);
            output.Add(
                $"EPalWazaID::{rowKey[prefix.Length..]}",
                new KoreanCatalogText(name, description));
        }
        return output;
    }

    private static SortedDictionary<string, KoreanCatalogText> BuildPassiveSkills(
        IReadOnlyDictionary<string, string> names,
        IReadOnlyDictionary<string, string> descriptions)
    {
        const string prefix = "PASSIVE_";
        var output = new SortedDictionary<string, KoreanCatalogText>(StringComparer.Ordinal);
        foreach (var (rowKey, name) in names.Where(
                     pair => pair.Key.StartsWith(prefix, StringComparison.Ordinal)))
        {
            var id = rowKey[prefix.Length..];
            string? description = null;
            foreach (var candidate in new[]
                     {
                         $"{rowKey}_DESC",
                         $"PASSIVE_PAL_{id}",
                         $"PASSIVE_PAL_{id}_DESC",
                         rowKey,
                     })
            {
                if (descriptions.TryGetValue(candidate, out description))
                {
                    break;
                }
            }
            output.TryAdd(id, new KoreanCatalogText(name, description));
        }
        return output;
    }

    private static SortedDictionary<string, string> ReadTextRows(
        UDataTable table,
        string prefix)
    {
        var allRows = ReadAllTextRows(table);
        return new SortedDictionary<string, string>(
            allRows
                .Where(pair => pair.Key.StartsWith(prefix, StringComparison.Ordinal))
                .ToDictionary(
                    pair => pair.Key[prefix.Length..],
                    pair => pair.Value,
                    StringComparer.Ordinal),
            StringComparer.Ordinal);
    }

    private static SortedDictionary<string, string> ReadAllTextRows(UDataTable table)
    {
        var output = new SortedDictionary<string, string>(StringComparer.Ordinal);
        foreach (var row in table.RowMap)
        {
            var textTag = row.Value.Properties.SingleOrDefault(
                property => property.Name.Text == "TextData")?.Tag;
            var text = textTag?.GetValue<FText>()?.Text?.Trim();
            if (string.IsNullOrWhiteSpace(text)
                || string.Equals(text, "ko_Text", StringComparison.Ordinal))
            {
                continue;
            }
            if (!output.TryAdd(row.Key.Text, text))
            {
                throw Failure($"duplicate Korean source row: {row.Key.Text}");
            }
        }
        return output;
    }

    private static string? ResolveCharacterNames(
        string? source,
        IReadOnlyDictionary<string, string> palNames)
    {
        if (source is null)
        {
            return null;
        }
        return CharacterNameMarkup().Replace(
            source,
            match => palNames.TryGetValue(match.Groups[1].Value, out var name)
                ? name
                : match.Value);
    }

    private static void RequireExactContract(AssetContract contract)
    {
        var expected = new Dictionary<string, int>(StringComparer.Ordinal)
        {
            [PalNamesPath] = 322,
            [PalDescriptionsPath] = 310,
            [SkillNamesPath] = 1157,
            [SkillDescriptionsPath] = 439,
        };
        if (contract.Tables.Count != expected.Count
            || contract.Tables.Any(table =>
                !expected.TryGetValue(table.PackagePath, out var count)
                || table.ExpectedRowCount != count
                || !table.RequiredProperties.SequenceEqual(["TextData"])))
        {
            throw Failure("Korean catalog contract does not pin the exact source tables");
        }
    }

    private static void AtomicWrite(string outputPath, ReadOnlySpan<byte> bytes)
    {
        var parent = Path.GetDirectoryName(outputPath)
            ?? throw Failure("Korean catalog output has no parent directory");
        Directory.CreateDirectory(parent);
        var temporaryPath = Path.Combine(
            parent,
            $".{Path.GetFileName(outputPath)}.{Guid.NewGuid():N}.tmp");
        try
        {
            File.WriteAllBytes(temporaryPath, bytes.ToArray());
            File.Move(temporaryPath, outputPath, overwrite: true);
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
