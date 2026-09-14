using System.Globalization;
using System.Text.Json;
using CUE4Parse.FileProvider;
using CUE4Parse.UE4.Assets.Exports.Engine;
using CUE4Parse.UE4.Assets.Objects.Properties;
using CUE4Parse.UE4.Objects.Core.i18N;

namespace PalDataPack;

public sealed record VerifiedPalBreedingWriteResult(
    string GameBuildId,
    string MappingSha256,
    string ContractReviewId,
    string OutputPath,
    string OutputSha256,
    int SourcePalRowCount,
    int DisplayPalCount,
    int BreedingSpeciesCount,
    int SpecialBreedingRuleCount,
    int GenderSpecificRuleCount);

public sealed record VerifiedPalBreedingDocument(
    int SchemaVersion,
    string GameBuildId,
    string MappingSha256,
    string ContractReviewId,
    bool Verified,
    string GeneralFormula,
    IReadOnlyList<PalBreedingSource> Sources,
    IReadOnlyList<VerifiedPalSourceRecord> SourcePalRows,
    IReadOnlyList<VerifiedPalDisplayRecord> Pals,
    IReadOnlyList<VerifiedBreedingSpecies> BreedingSpecies,
    IReadOnlyList<VerifiedSpecialBreedingRule> SpecialBreeding,
    IReadOnlyList<VerifiedSpecialBreedingRule> PaldexSpecialBreeding);

public sealed record PalBreedingSource(
    string Capability,
    string PackagePath,
    int RowCount);

public sealed record VerifiedPalWorkSuitability(
    string WorkTypeId,
    int Level);

public sealed record VerifiedPalSourceRecord(
    string SourceRowId,
    string InternalId,
    string LocalizationKey,
    string? NameKo,
    string? NameEn,
    string? DescriptionKo,
    string? DescriptionEn,
    int PaldexNumber,
    string PaldexSuffix,
    bool IsPal,
    bool IsBoss,
    bool IsRaidBoss,
    bool IsTowerBoss,
    bool IsPredator,
    bool UseBossHpGauge,
    bool Nocturnal,
    IReadOnlyList<string> Elements,
    int Hp,
    int Attack,
    int Defense,
    int WalkSpeed,
    int RunSpeed,
    int RideSprintSpeed,
    int TransportSpeed,
    int Stamina,
    int FoodAmount,
    int Rarity,
    int CombiRank,
    int CombiDuplicatePriority,
    bool IgnoreCombi,
    IReadOnlyList<VerifiedPalWorkSuitability> WorkSuitability,
    IReadOnlyList<string> GuaranteedPassiveSkillIds);

public sealed record VerifiedPalDisplayRecord(
    string InternalId,
    string SourceRowId,
    string LocalizationKey,
    string NameKo,
    string? NameEn,
    string? DescriptionKo,
    string? DescriptionEn,
    int PaldexNumber,
    string PaldexSuffix,
    bool IsBoss,
    bool IsRaidBoss,
    bool IsTowerBoss,
    bool IsPredator,
    bool UseBossHpGauge,
    bool Nocturnal,
    IReadOnlyList<string> Elements,
    int Hp,
    int Attack,
    int Defense,
    int WalkSpeed,
    int RunSpeed,
    int RideSprintSpeed,
    int TransportSpeed,
    int Stamina,
    int FoodAmount,
    int Rarity,
    int CombiRank,
    int CombiDuplicatePriority,
    bool IgnoreCombi,
    IReadOnlyList<VerifiedPalWorkSuitability> WorkSuitability,
    IReadOnlyList<string> GuaranteedPassiveSkillIds,
    IReadOnlyList<string> SourceRowIds);

public sealed record VerifiedBreedingSpecies(
    string InternalId,
    string NameKo,
    string? NameEn,
    int PaldexNumber,
    string PaldexSuffix,
    int CombiRank,
    int CombiDuplicatePriority,
    bool IgnoreCombi,
    string SourceRowId);

public sealed record VerifiedSpecialBreedingRule(
    string RuleId,
    string ParentAInternalId,
    string ParentAGender,
    string ParentBInternalId,
    string ParentBGender,
    string ChildInternalId,
    IReadOnlyList<string> SourceRowIds);

public static class VerifiedPalBreedingExtractor
{
    private const string PalsPath =
        "Pal/Content/Pal/DataTable/Character/DT_PalMonsterParameter";
    private const string SpecialBreedingPath =
        "Pal/Content/Pal/DataTable/Character/DT_PalCombiUnique";
    private const string KoreanNamesPath =
        "Pal/Content/L10N/ko/Pal/DataTable/Text/DT_PalNameText_Common";
    private const string KoreanDescriptionsPath =
        "Pal/Content/L10N/ko/Pal/DataTable/Text/DT_PalLongDescriptionText";
    private const string EnglishNamesPath =
        "Pal/Content/L10N/en/Pal/DataTable/Text/DT_PalNameText_Common";
    private const string EnglishDescriptionsPath =
        "Pal/Content/L10N/en/Pal/DataTable/Text/DT_PalLongDescriptionText";
    private const int MaximumOutputBytes = 32 * 1024 * 1024;

    private static readonly string[] WorkTypes =
    [
        "Collection",
        "Cool",
        "Deforest",
        "EmitFlame",
        "GenerateElectricity",
        "Handcraft",
        "Mining",
        "MonsterFarm",
        "OilExtraction",
        "ProductMedicine",
        "Seeding",
        "Transport",
        "Watering",
    ];

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        WriteIndented = false,
    };

    public static VerifiedPalBreedingWriteResult ExtractToFile(
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
            phase = "localization";
            var namesKo = ReadLocalization(tables[KoreanNamesPath], "PAL_NAME_");
            var namesEn = ReadLocalization(tables[EnglishNamesPath], "PAL_NAME_");
            var descriptionsKo = ReadLocalization(
                tables[KoreanDescriptionsPath],
                "PAL_LONG_DESC_");
            var descriptionsEn = ReadLocalization(
                tables[EnglishDescriptionsPath],
                "PAL_LONG_DESC_");
            phase = "Pal rows";
            var sourceRows = ReadPalRows(
                tables[PalsPath],
                namesKo,
                namesEn,
                descriptionsKo,
                descriptionsEn);
            phase = "display projection";
            var displayPals = BuildDisplayPals(sourceRows);
            var knownSpecies = sourceRows
                .Where(row => row.IsPal)
                .Select(row => row.InternalId)
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .ToDictionary(
                    internalId => internalId,
                    internalId => internalId,
                    StringComparer.OrdinalIgnoreCase);
            phase = "breeding species";
            var breedingSpecies = BuildBreedingSpecies(displayPals);
            phase = "special breeding";
            var specialBreeding = ReadSpecialBreeding(
                tables[SpecialBreedingPath],
                knownSpecies);
            var paldexIds = displayPals
                .Select(pal => pal.InternalId)
                .ToHashSet(StringComparer.OrdinalIgnoreCase);
            var paldexSpecialBreeding = specialBreeding
                .Where(rule =>
                    paldexIds.Contains(rule.ParentAInternalId)
                    && paldexIds.Contains(rule.ParentBInternalId)
                    && paldexIds.Contains(rule.ChildInternalId))
                .ToArray();

            var document = new VerifiedPalBreedingDocument(
                SchemaVersion: 1,
                GameBuildId: $"steam:{install.BuildId}",
                MappingSha256: mappingSha256,
                ContractReviewId: contract.ReviewId,
                Verified: true,
                GeneralFormula:
                    "floor((parent_a_combi_rank + parent_b_combi_rank + 1) / 2); "
                    + "exclude special-only children; nearest eligible child by "
                    + "absolute rank distance, then descending "
                    + "combi_duplicate_priority, then internal_id; special rules first",
                Sources: contract.Tables
                    .OrderBy(table => table.PackagePath, StringComparer.Ordinal)
                    .Select(table => new PalBreedingSource(
                        table.Capability,
                        table.PackagePath,
                        tables[table.PackagePath].RowMap.Count))
                    .ToArray(),
                SourcePalRows: sourceRows,
                Pals: displayPals,
                BreedingSpecies: breedingSpecies,
                SpecialBreeding: specialBreeding,
                PaldexSpecialBreeding: paldexSpecialBreeding);
            var bytes = JsonSerializer.SerializeToUtf8Bytes(document, JsonOptions);
            if (bytes.Length > MaximumOutputBytes)
            {
                throw Failure("verified Pal and breeding output exceeds its bound");
            }
            var writtenPath = AtomicWrite(outputPath, bytes);
            return new VerifiedPalBreedingWriteResult(
                document.GameBuildId,
                mappingSha256,
                contract.ReviewId,
                writtenPath,
                Hashing.Sha256Hex(bytes),
                sourceRows.Count,
                displayPals.Count,
                breedingSpecies.Count,
                specialBreeding.Count,
                specialBreeding.Count(rule =>
                    rule.ParentAGender != "None"
                    || rule.ParentBGender != "None"));
        }
        catch (DataPackFailure)
        {
            throw;
        }
        catch (Exception error)
        {
            throw new DataPackFailure(
                DataPackExitCode.MountOrSerialization,
                $"exact-Build Pal and breeding extraction failed during {phase} "
                + $"({error.GetType().Name})");
        }
    }

    public static int GeneralTargetRank(int parentARank, int parentBRank) =>
        checked((parentARank + parentBRank + 1) / 2);

    public static VerifiedBreedingSpecies GeneralChild(
        int parentARank,
        int parentBRank,
        IEnumerable<VerifiedBreedingSpecies> species,
        IReadOnlySet<string>? specialOnlyChildIds = null)
    {
        var target = GeneralTargetRank(parentARank, parentBRank);
        return species
            .Where(candidate =>
                !candidate.IgnoreCombi
                &&
                specialOnlyChildIds?.Contains(candidate.InternalId) != true)
            .OrderBy(candidate => Math.Abs((long)candidate.CombiRank - target))
            .ThenByDescending(candidate => candidate.CombiDuplicatePriority)
            .ThenBy(candidate => candidate.InternalId, StringComparer.Ordinal)
            .FirstOrDefault()
            ?? throw Failure("general breeding requires at least one eligible species");
    }

    private static IReadOnlyList<VerifiedPalSourceRecord> ReadPalRows(
        UDataTable table,
        IReadOnlyDictionary<string, string> namesKo,
        IReadOnlyDictionary<string, string> namesEn,
        IReadOnlyDictionary<string, string> descriptionsKo,
        IReadOnlyDictionary<string, string> descriptionsEn)
    {
        var output = new List<VerifiedPalSourceRecord>(table.RowMap.Count);
        foreach (var row in table.RowMap.OrderBy(
                     row => row.Key.Text,
                     StringComparer.Ordinal))
        {
            try
            {
                var sourceRowId = RequireIdentifier(row.Key.Text, "Pal source row");
                var properties = Properties(row.Value);
                var internalId = StripEnum(Name(properties, "Tribe"));
                RequireIdentifier(internalId, "Pal internal ID");
                var overrideName = Name(properties, "OverrideNameTextID");
                var localizationKey = overrideName == "None"
                    ? internalId
                    : StripPrefix(overrideName, "PAL_NAME_");
                RequireIdentifier(localizationKey, "Pal localization key");
                namesKo.TryGetValue(localizationKey, out var nameKo);
                namesEn.TryGetValue(localizationKey, out var nameEn);
                descriptionsKo.TryGetValue(localizationKey, out var descriptionKo);
                descriptionsEn.TryGetValue(localizationKey, out var descriptionEn);

                output.Add(new VerifiedPalSourceRecord(
                    sourceRowId,
                    internalId,
                    localizationKey,
                    NullIfBlank(nameKo),
                    NullIfBlank(nameEn),
                    NullIfBlank(descriptionKo),
                    NullIfBlank(descriptionEn),
                    Int(properties, "ZukanIndex"),
                    StringValue(properties, "ZukanIndexSuffix"),
                    Bool(properties, "IsPal"),
                    Bool(properties, "IsBoss"),
                    Bool(properties, "IsRaidBoss"),
                    Bool(properties, "IsTowerBoss"),
                    Bool(properties, "Predator"),
                    Bool(properties, "UseBossHPGauge"),
                    Bool(properties, "Nocturnal"),
                    Elements(properties),
                    NonNegative(Int(properties, "Hp"), "Pal HP"),
                    NonNegative(Int(properties, "ShotAttack"), "Pal attack"),
                    NonNegative(Int(properties, "Defense"), "Pal defense"),
                    Int(properties, "WalkSpeed"),
                    Int(properties, "RunSpeed"),
                    Int(properties, "RideSprintSpeed"),
                    Int(properties, "TransportSpeed"),
                    NonNegative(Int(properties, "Stamina"), "Pal stamina"),
                    NonNegative(Int(properties, "FoodAmount"), "Pal food amount"),
                    NonNegative(Int(properties, "Rarity"), "Pal rarity"),
                    NonNegative(Int(properties, "CombiRank"), "Pal CombiRank"),
                    NonNegative(
                        Int(properties, "CombiDuplicatePriority"),
                        "Pal CombiDuplicatePriority"),
                    Bool(properties, "IgnoreCombi"),
                    WorkSuitability(properties),
                    PassiveSkills(properties)));
            }
            catch (DataPackFailure error)
            {
                throw Failure(
                    $"Pal source row is invalid: {row.Key.Text}; {error.Message}");
            }
            catch (Exception error)
            {
                throw Failure(
                    $"Pal source row could not be decoded: {row.Key.Text} "
                    + $"({error.GetType().Name})");
            }
        }
        return output;
    }

    private static IReadOnlyList<VerifiedPalDisplayRecord> BuildDisplayPals(
        IReadOnlyList<VerifiedPalSourceRecord> sourceRows)
    {
        var output = new List<VerifiedPalDisplayRecord>();
        foreach (var group in sourceRows
                     .Where(row => row.IsPal)
                     .GroupBy(row => row.InternalId, StringComparer.OrdinalIgnoreCase)
                     .OrderBy(group => group.Key, StringComparer.Ordinal))
        {
            var projectionCandidates = group
                .Where(row =>
                    row.PaldexNumber > 0
                    && row.NameKo is not null)
                .OrderByDescending(row =>
                    string.Equals(
                        row.SourceRowId,
                        row.InternalId,
                        StringComparison.Ordinal))
                .ThenBy(row => row.IsBoss)
                .ThenBy(row => row.IsRaidBoss)
                .ThenBy(row => row.IsTowerBoss)
                .ThenBy(row => row.SourceRowId, StringComparer.Ordinal)
                .ToArray();
            if (projectionCandidates.Length == 0)
            {
                continue;
            }
            var selected = projectionCandidates[0];
            var conflicting = projectionCandidates.Where(row =>
                row.CombiRank != selected.CombiRank
                || row.CombiDuplicatePriority != selected.CombiDuplicatePriority
                || row.IgnoreCombi != selected.IgnoreCombi).ToArray();
            if (conflicting.Length > 0
                && !string.Equals(
                    selected.SourceRowId,
                    selected.InternalId,
                    StringComparison.Ordinal))
            {
                throw Failure(
                    $"Pal canonical breeding fields conflict without an exact row: "
                    + selected.InternalId);
            }
            output.Add(new VerifiedPalDisplayRecord(
                selected.InternalId,
                selected.SourceRowId,
                selected.LocalizationKey,
                selected.NameKo!,
                selected.NameEn,
                selected.DescriptionKo,
                selected.DescriptionEn,
                selected.PaldexNumber,
                selected.PaldexSuffix,
                selected.IsBoss,
                selected.IsRaidBoss,
                selected.IsTowerBoss,
                selected.IsPredator,
                selected.UseBossHpGauge,
                selected.Nocturnal,
                selected.Elements,
                selected.Hp,
                selected.Attack,
                selected.Defense,
                selected.WalkSpeed,
                selected.RunSpeed,
                selected.RideSprintSpeed,
                selected.TransportSpeed,
                selected.Stamina,
                selected.FoodAmount,
                selected.Rarity,
                selected.CombiRank,
                selected.CombiDuplicatePriority,
                selected.IgnoreCombi,
                selected.WorkSuitability,
                selected.GuaranteedPassiveSkillIds,
                group
                    .Select(row => row.SourceRowId)
                    .Order(StringComparer.Ordinal)
                    .ToArray()));
        }
        return output;
    }

    private static IReadOnlyList<VerifiedBreedingSpecies> BuildBreedingSpecies(
        IReadOnlyList<VerifiedPalDisplayRecord> pals) =>
        pals
            .Where(pal =>
                pal.CombiRank < 999_999
                && pal.PaldexNumber > 0)
            .Select(pal => new VerifiedBreedingSpecies(
                pal.InternalId,
                pal.NameKo,
                pal.NameEn,
                pal.PaldexNumber,
                pal.PaldexSuffix,
                pal.CombiRank,
                pal.CombiDuplicatePriority,
                pal.IgnoreCombi,
                pal.SourceRowId))
            .OrderBy(pal => pal.InternalId, StringComparer.Ordinal)
            .ToArray();

    private static IReadOnlyList<VerifiedSpecialBreedingRule> ReadSpecialBreeding(
        UDataTable table,
        IReadOnlyDictionary<string, string> knownSpecies)
    {
        var output = new Dictionary<string, VerifiedSpecialBreedingRule>(
            StringComparer.Ordinal);
        foreach (var row in table.RowMap.OrderBy(
                     row => row.Key.Text,
                     StringComparer.Ordinal))
        {
            var properties = Properties(row.Value);
            var sourceRowId = RequireIdentifier(row.Key.Text, "special breeding row");
            var parentA = RequireKnownSpecies(
                StripEnum(Name(properties, "ParentTribeA")),
                knownSpecies,
                "special parent A");
            var parentB = RequireKnownSpecies(
                StripEnum(Name(properties, "ParentTribeB")),
                knownSpecies,
                "special parent B");
            var genderA = StripEnum(Name(properties, "ParentGenderA"));
            var genderB = StripEnum(Name(properties, "ParentGenderB"));
            var child = RequireKnownSpecies(
                Name(properties, "ChildCharacterID"),
                knownSpecies,
                "special child");
            if (string.CompareOrdinal(parentA, parentB) > 0)
            {
                (parentA, parentB) = (parentB, parentA);
                (genderA, genderB) = (genderB, genderA);
            }
            var semanticKey =
                $"{parentA}\u001f{genderA}\u001f{parentB}\u001f{genderB}\u001f{child}";
            if (output.TryGetValue(semanticKey, out var existing))
            {
                output[semanticKey] = existing with
                {
                    SourceRowIds = existing.SourceRowIds
                        .Append(sourceRowId)
                        .Order(StringComparer.Ordinal)
                        .ToArray(),
                };
                continue;
            }
            output.Add(semanticKey, new VerifiedSpecialBreedingRule(
                $"special:{sourceRowId}",
                parentA,
                genderA,
                parentB,
                genderB,
                child,
                [sourceRowId]));
        }
        return output.Values
            .OrderBy(rule => rule.ParentAInternalId, StringComparer.Ordinal)
            .ThenBy(rule => rule.ParentBInternalId, StringComparer.Ordinal)
            .ThenBy(rule => rule.ChildInternalId, StringComparer.Ordinal)
            .ThenBy(rule => rule.SourceRowIds[0], StringComparer.Ordinal)
            .ToArray();
    }

    private static IReadOnlyList<string> Elements(
        IReadOnlyDictionary<string, FPropertyTagType?> properties)
    {
        var output = new List<string>(2);
        foreach (var property in new[] { "ElementType1", "ElementType2" })
        {
            var value = StripEnum(Name(properties, property));
            if (value != "None" && !output.Contains(value, StringComparer.Ordinal))
            {
                output.Add(value);
            }
        }
        return output;
    }

    private static IReadOnlyList<VerifiedPalWorkSuitability> WorkSuitability(
        IReadOnlyDictionary<string, FPropertyTagType?> properties) =>
        WorkTypes
            .Select(workType => new VerifiedPalWorkSuitability(
                workType,
                NonNegative(
                    Int(properties, $"WorkSuitability_{workType}"),
                    $"Pal work suitability {workType}")))
            .Where(work => work.Level > 0)
            .ToArray();

    private static IReadOnlyList<string> PassiveSkills(
        IReadOnlyDictionary<string, FPropertyTagType?> properties)
    {
        var output = new List<string>(4);
        for (var slot = 1; slot <= 4; slot++)
        {
            var value = Name(properties, $"PassiveSkill{slot}");
            if (value != "None" && !output.Contains(value, StringComparer.Ordinal))
            {
                RequireIdentifier(value, "guaranteed passive skill");
                output.Add(value);
            }
        }
        return output;
    }

    private static Dictionary<string, string> ReadLocalization(
        UDataTable table,
        string prefix)
    {
        var output = new Dictionary<string, string>(
            table.RowMap.Count,
            StringComparer.OrdinalIgnoreCase);
        foreach (var row in table.RowMap)
        {
            if (!row.Key.Text.StartsWith(prefix, StringComparison.Ordinal))
            {
                continue;
            }
            var key = row.Key.Text[prefix.Length..];
            var text = RequiredTag(Properties(row.Value), "TextData")
                .GetValue<FText>()?.Text?.Trim();
            if (string.IsNullOrWhiteSpace(key)
                || string.IsNullOrWhiteSpace(text)
                || !output.TryAdd(key, text))
            {
                throw Failure($"localized Pal text is invalid: {row.Key.Text}");
            }
        }
        return output;
    }

    private static Dictionary<string, UDataTable> LoadAndValidateTables(
        DefaultFileProvider provider,
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
                    $"exact row count is not pinned: {tableContract.PackagePath}");
            }
            var observed = table.RowMap.Values
                .SelectMany(row => row.Properties)
                .Select(property => property.Name.Text)
                .ToHashSet(StringComparer.Ordinal);
            if (!tableContract.RequiredProperties.All(observed.Contains))
            {
                throw Failure(
                    $"contracted table schema drift: {tableContract.PackagePath}");
            }
            tables.Add(tableContract.PackagePath, table);
        }
        return tables;
    }

    private static Dictionary<string, FPropertyTagType?> Properties(
        CUE4Parse.UE4.Assets.Objects.FStructFallback row) =>
        row.Properties.ToDictionary(
            property => property.Name.Text,
            property => property.Tag,
            StringComparer.Ordinal);

    private static FPropertyTagType RequiredTag(
        IReadOnlyDictionary<string, FPropertyTagType?> properties,
        string name) =>
        properties.TryGetValue(name, out var tag) && tag is not null
            ? tag
            : throw Failure($"required Pal property is missing: {name}");

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
            : throw Failure($"required name property is empty: {name}");
    }

    private static int Int(
        IReadOnlyDictionary<string, FPropertyTagType?> properties,
        string name)
    {
        var value = StringValue(properties, name);
        return int.TryParse(
            value,
            NumberStyles.Integer,
            CultureInfo.InvariantCulture,
            out var parsed)
            ? parsed
            : throw Failure($"required integer property is invalid: {name}");
    }

    private static bool Bool(
        IReadOnlyDictionary<string, FPropertyTagType?> properties,
        string name)
    {
        var value = StringValue(properties, name);
        return bool.TryParse(value, out var parsed)
            ? parsed
            : throw Failure($"required Boolean property is invalid: {name}");
    }

    private static string RequireKnownSpecies(
        string value,
        IReadOnlyDictionary<string, string> knownSpecies,
        string relation)
    {
        RequireIdentifier(value, relation);
        return knownSpecies.TryGetValue(value, out var canonical)
            ? canonical
            : throw Failure($"{relation} references an unknown Pal: {value}");
    }

    private static string StripEnum(string value)
    {
        var separator = value.IndexOf("::", StringComparison.Ordinal);
        return separator >= 0 ? value[(separator + 2)..] : value;
    }

    private static string StripPrefix(string value, string prefix) =>
        value.StartsWith(prefix, StringComparison.Ordinal)
            ? value[prefix.Length..]
            : value;

    private static string? NullIfBlank(string? value) =>
        string.IsNullOrWhiteSpace(value) ? null : value.Trim();

    private static string RequireIdentifier(string value, string field)
    {
        CatalogContract.RequireIdentifier(value, field);
        return value;
    }

    private static int NonNegative(int value, string field) =>
        value >= 0 ? value : throw Failure($"{field} cannot be negative");

    private static void RequireExactContract(AssetContract contract)
    {
        var expected = new Dictionary<string, int>(StringComparer.Ordinal)
        {
            [PalsPath] = 753,
            [SpecialBreedingPath] = 258,
            [KoreanNamesPath] = 322,
            [KoreanDescriptionsPath] = 310,
            [EnglishNamesPath] = 322,
            [EnglishDescriptionsPath] = 310,
        };
        if (contract.Tables.Count != expected.Count
            || contract.Tables.Any(table =>
                !table.Required
                || !expected.TryGetValue(table.PackagePath, out var count)
                || table.ExpectedRowCount != count))
        {
            throw Failure(
                "Pal and breeding extraction requires the closed six-table contract");
        }
    }

    private static string AtomicWrite(string outputPath, ReadOnlySpan<byte> bytes)
    {
        var fullPath = Path.GetFullPath(outputPath);
        var directory = Path.GetDirectoryName(fullPath)
            ?? throw Failure("Pal and breeding output directory is invalid");
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
