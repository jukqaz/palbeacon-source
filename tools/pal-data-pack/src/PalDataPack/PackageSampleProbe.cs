using System.Globalization;
using System.Text.Json;
using CUE4Parse.UE4.Assets.Objects;
using CUE4Parse.UE4.Assets.Objects.Properties;
using CUE4Parse.UE4.Assets.Exports;
using CUE4Parse.UE4.Objects.Core.i18N;
using CUE4Parse.UE4.Objects.UObject;
using NewtonsoftJson = Newtonsoft.Json.JsonConvert;

namespace PalDataPack;

public sealed record PackageObjectSample(
    string Name,
    string ObjectPath,
    string RuntimeType,
    IReadOnlyDictionary<string, string> Properties,
    string? ScriptBytecode);

public sealed record PackageSampleProbeResult(
    string GameBuildId,
    string MappingSha256,
    string PackagePath,
    IReadOnlyList<PackageObjectSample> Objects);

public static class PackageSampleProbe
{
    public const string ItemRarityWidgetPackagePath =
        "Pal/Content/Pal/Blueprint/UI/UserInterface/MainMenu/"
        + "InventoryEquipment/WBP_InventoryEquipment_ItemInfo";

    public static PackageSampleProbeResult Probe(
        SteamInstall install,
        string mappingPath,
        AssetContract contract,
        string packagePath)
    {
        ValidatePackagePath(packagePath);
        SecureInput.EnsureRegularFile(mappingPath);
        var mappingSha256 = Hashing.Sha256File(mappingPath);
        contract.RequireProbeIdentity(install.BuildId, mappingSha256);

        try
        {
            var provider = SourceContractProbe.OpenProvider(
                install.InstallPath,
                mappingPath);
            provider.ReadScriptData = true;
            var objects = provider.LoadPackage(packagePath)
                .GetExports()
                .Take(1024)
                .Select(value => new PackageObjectSample(
                    value.Name ?? "<unnamed>",
                    value.GetPathName(),
                    value.GetType().FullName ?? value.GetType().Name,
                    (value.Properties ?? [])
                        .OrderBy(property => property.Name.Text, StringComparer.Ordinal)
                        .ToDictionary(
                            property => property.ArrayIndex > 0
                                ? $"{property.Name.Text}[{property.ArrayIndex}]"
                                : property.Name.Text,
                            property => BoundedValue(property.Tag),
                            StringComparer.Ordinal),
                    value is UStruct { ScriptBytecode.Length: > 0 } script
                        ? NewtonsoftJson.SerializeObject(script.ScriptBytecode)
                        : null))
                .ToArray();
            return new PackageSampleProbeResult(
                install.BuildId,
                mappingSha256,
                packagePath,
                objects);
        }
        catch (DataPackFailure)
        {
            throw;
        }
        catch (Exception error)
        {
            throw new DataPackFailure(
                DataPackExitCode.MountOrSerialization,
                $"CUE4Parse could not sample exact-Build package: {error}");
        }
    }

    public static void ValidatePackagePath(string packagePath)
    {
        if (!string.Equals(
                packagePath,
                ItemRarityWidgetPackagePath,
                StringComparison.Ordinal))
        {
            throw new DataPackFailure(
                DataPackExitCode.Usage,
                "package sampling is limited to the reviewed item-rarity "
                + "widget package");
        }
    }

    private static string BoundedValue(FPropertyTagType? tag)
    {
        var value = NormalizeTag(tag, depth: 0);
        var text = value is string scalar
            ? scalar
            : JsonSerializer.Serialize(value);
        return new string(text
            .Where(character => !char.IsControl(character) || character is '\t')
            .Take(16 * 1024)
            .ToArray());
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
        var converted = Convert.ToString(
            tag.GenericValue,
            CultureInfo.InvariantCulture) ?? string.Empty;
        if (converted.Contains("FMovieSceneChannel", StringComparison.Ordinal))
        {
            return NewtonsoftJson.SerializeObject(tag.GenericValue);
        }
        return converted;
    }
}
