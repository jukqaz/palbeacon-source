using System.Text.Json;
using CUE4Parse.UE4.Assets.Exports.Engine;
using CUE4Parse.UE4.Objects.Core.i18N;

namespace PalDataPack;

public sealed record VerifiedKoreanUiWriteResult(
    string GameBuildId,
    string MappingSha256,
    string ContractReviewId,
    string OutputPath,
    string OutputSha256,
    int ElementCount,
    int WorkSuitabilityCount,
    int ItemCategoryCount);

public sealed record VerifiedKoreanUiDocument(
    int SchemaVersion,
    string GameBuildId,
    string MappingSha256,
    string ContractReviewId,
    bool Verified,
    string Language,
    string LocalizationOrigin,
    bool MachineTranslationAllowed,
    KoreanUiSource Source,
    IReadOnlyDictionary<string, KoreanUiText> Elements,
    IReadOnlyDictionary<string, KoreanUiText> WorkSuitability,
    IReadOnlyDictionary<string, KoreanUiText> ItemCategories);

public sealed record KoreanUiSource(
    string Capability,
    string PackagePath,
    int RowCount);

public sealed record KoreanUiText(
    string SourceRowKey,
    string NameKo);

public static class VerifiedKoreanUiExtractor
{
    private const string KoreanUiPath =
        "Pal/Content/L10N/ko/Pal/DataTable/Text/DT_UI_Common_Text_Common";
    private const int MaximumOutputBytes = 64 * 1024;

    private static readonly IReadOnlyDictionary<string, string> ElementRows =
        new Dictionary<string, string>(StringComparer.Ordinal)
        {
            ["Neutral"] = "COMMON_ELEMENT_NAME_Normal",
            ["Fire"] = "COMMON_ELEMENT_NAME_Fire",
            ["Water"] = "COMMON_ELEMENT_NAME_Water",
            ["Electricity"] = "COMMON_ELEMENT_NAME_Electricity",
            ["Leaf"] = "COMMON_ELEMENT_NAME_Leaf",
            ["Ice"] = "COMMON_ELEMENT_NAME_Ice",
            ["Earth"] = "COMMON_ELEMENT_NAME_Earth",
            ["Dark"] = "COMMON_ELEMENT_NAME_Dark",
            ["Dragon"] = "COMMON_ELEMENT_NAME_Dragon",
        };

    private static readonly IReadOnlyDictionary<string, string> WorkRows =
        new Dictionary<string, string>(StringComparer.Ordinal)
        {
            ["EmitFlame"] = "COMMON_WORK_SUITABILITY_EmitFlame",
            ["Watering"] = "COMMON_WORK_SUITABILITY_Watering",
            ["Seeding"] = "COMMON_WORK_SUITABILITY_Seeding",
            ["GenerateElectricity"] = "COMMON_WORK_SUITABILITY_GenerateElectricity",
            ["Handcraft"] = "COMMON_WORK_SUITABILITY_Handcraft",
            ["Collection"] = "COMMON_WORK_SUITABILITY_Collection",
            ["Deforest"] = "COMMON_WORK_SUITABILITY_Deforest",
            ["Mining"] = "COMMON_WORK_SUITABILITY_Mining",
            ["OilExtraction"] = "COMMON_WORK_SUITABILITY_OilExtraction",
            ["ProductMedicine"] = "COMMON_WORK_SUITABILITY_ProductMedicine",
            ["Cool"] = "COMMON_WORK_SUITABILITY_Cool",
            ["Transport"] = "COMMON_WORK_SUITABILITY_Transport",
            ["MonsterFarm"] = "COMMON_WORK_SUITABILITY_MonsterFarm",
        };

    private static readonly IReadOnlyDictionary<string, string> ItemCategoryRows =
        new Dictionary<string, string>(StringComparer.Ordinal)
        {
            ["Accessory"] = "COMMON_ITEMTYPE_A_Accessory",
            ["Ammo"] = "COMMON_ITEMTYPE_A_Ammo",
            ["Armor"] = "COMMON_ITEMTYPE_A_Armor",
            ["Blueprint"] = "COMMON_ITEMTYPE_A_Blueprint",
            ["CaptureItemModifier"] = "COMMON_ITEMTYPE_A_CaptureItemModifier",
            ["Consume"] = "COMMON_ITEMTYPE_A_Consume",
            ["Essential"] = "COMMON_ITEMTYPE_A_Essential",
            ["Food"] = "COMMON_ITEMTYPE_A_Food",
            ["Glider"] = "COMMON_ITEMTYPE_A_Glider",
            ["Material"] = "COMMON_ITEMTYPE_A_Material",
            ["SpecialWeapon"] = "COMMON_ITEMTYPE_A_SpecialWeapon",
            ["Weapon"] = "COMMON_ITEMTYPE_A_Weapon",
        };

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        WriteIndented = true,
    };

    public static VerifiedKoreanUiWriteResult ExtractToFile(
        SteamInstall install,
        string mappingPath,
        AssetContract contract,
        string outputPath)
    {
        SecureInput.EnsureRegularFile(mappingPath);
        var mappingSha256 = Hashing.Sha256File(mappingPath);
        contract.RequireExtractionIdentity(install.BuildId, mappingSha256);
        RequireExactContract(contract);
        var expectedRowCount = contract.Tables[0].ExpectedRowCount!.Value;

        try
        {
            var provider = SourceContractProbe.OpenProvider(
                install.InstallPath,
                mappingPath);
            var table = provider.LoadPackageObject<UDataTable>(KoreanUiPath);
            if (table.RowMap.Count != expectedRowCount)
            {
                throw Failure("Korean UI table row count drifted");
            }

            var elements = ReadExactRows(table, ElementRows);
            var workSuitability = ReadExactRows(table, WorkRows);
            var itemCategories = ReadExactRows(table, ItemCategoryRows);
            var document = new VerifiedKoreanUiDocument(
                SchemaVersion: 1,
                GameBuildId: $"steam:{install.BuildId}",
                MappingSha256: mappingSha256,
                ContractReviewId: contract.ReviewId,
                Verified: true,
                Language: "ko",
                LocalizationOrigin: "game_l10n",
                MachineTranslationAllowed: false,
                Source: new KoreanUiSource(
                    "localization",
                    KoreanUiPath,
                    table.RowMap.Count),
                Elements: elements,
                WorkSuitability: workSuitability,
                ItemCategories: itemCategories);
            var bytes = JsonSerializer.SerializeToUtf8Bytes(document, JsonOptions);
            if (bytes.Length > MaximumOutputBytes)
            {
                throw Failure("verified Korean UI catalog exceeds its output bound");
            }
            var writtenPath = AtomicWrite(outputPath, bytes);
            return new VerifiedKoreanUiWriteResult(
                document.GameBuildId,
                mappingSha256,
                contract.ReviewId,
                writtenPath,
                Hashing.Sha256Hex(bytes),
                elements.Count,
                workSuitability.Count,
                itemCategories.Count);
        }
        catch (DataPackFailure)
        {
            throw;
        }
        catch (Exception error)
        {
            throw new DataPackFailure(
                DataPackExitCode.MountOrSerialization,
                $"exact-Build Korean UI extraction failed ({error.GetType().Name})");
        }
    }

    private static IReadOnlyDictionary<string, KoreanUiText> ReadExactRows(
        UDataTable table,
        IReadOnlyDictionary<string, string> requiredRows)
    {
        var rows = table.RowMap.ToDictionary(
            row => row.Key.Text,
            row => row.Value,
            StringComparer.Ordinal);
        var result = new SortedDictionary<string, KoreanUiText>(StringComparer.Ordinal);
        foreach (var (id, rowKey) in requiredRows)
        {
            if (!rows.TryGetValue(rowKey, out var row))
            {
                throw Failure($"required Korean UI row is missing: {rowKey}");
            }
            var textTag = row.Properties.SingleOrDefault(
                property => property.Name.Text == "TextData")?.Tag;
            var text = textTag?.GetValue<FText>()?.Text?.Trim();
            if (string.IsNullOrWhiteSpace(text)
                || string.Equals(text, "ko_Text", StringComparison.Ordinal))
            {
                throw Failure($"required Korean UI text is invalid: {rowKey}");
            }
            result.Add(id, new KoreanUiText(rowKey, text));
        }
        return result;
    }

    private static void RequireExactContract(AssetContract contract)
    {
        if (contract.Tables.Count != 1
            || contract.Tables[0].PackagePath != KoreanUiPath
            || contract.Tables[0].ExpectedRowCount is not > 0
            || !contract.Tables[0].RequiredProperties.SequenceEqual(["TextData"]))
        {
            throw Failure("Korean UI extraction contract does not pin the exact source table");
        }
    }

    private static string AtomicWrite(string outputPath, ReadOnlySpan<byte> bytes)
    {
        var fullPath = Path.GetFullPath(outputPath);
        var parent = Path.GetDirectoryName(fullPath)
            ?? throw Failure("Korean UI output has no parent directory");
        Directory.CreateDirectory(parent);
        var temporaryPath = Path.Combine(
            parent,
            $".{Path.GetFileName(fullPath)}.{Guid.NewGuid():N}.tmp");
        try
        {
            File.WriteAllBytes(temporaryPath, bytes.ToArray());
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
