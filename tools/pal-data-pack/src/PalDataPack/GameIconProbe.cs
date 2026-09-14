using System.Text;

namespace PalDataPack;

public sealed record GameIconGroupDefinition(
    string GroupId,
    string AssetRoot,
    string FileNamePrefix,
    string FileNameSuffix);

public sealed record GameIconGroupProbe(
    string GroupId,
    string AssetRoot,
    string FileNamePrefix,
    string FileNameSuffix,
    int TextureCount,
    string InventorySha256,
    IReadOnlyList<string> PackagePaths);

public sealed record GameIconProbeResult(
    string GameBuildId,
    string MappingSha256,
    IReadOnlyList<GameIconGroupProbe> Groups);

public static class GameIconProbe
{
    public static readonly IReadOnlyList<GameIconGroupDefinition> Definitions =
    [
        new(
            "pal_portrait",
            "Pal/Content/Pal/Texture/PalIcon/Normal/",
            "T_",
            "_icon_normal"),
        new(
            "element",
            "Pal/Content/Pal/Texture/UI/Main_Menu/",
            "T_Icon_element_",
            ""),
        new(
            "work_suitability",
            "Pal/Content/Pal/Texture/UI/InGame/",
            "T_icon_palwork_",
            ""),
        new(
            "skill",
            "Pal/Content/Pal/Texture/UI/InGame/SkillIcon/",
            "T_icon_skill_pal_",
            ""),
        new(
            "rarity",
            "Pal/Content/Pal/Texture/UI/InGame/",
            "T_icon_enemy_rarity_",
            ""),
        new(
            "item_category",
            "Pal/Content/Pal/Texture/UI/IngameMenu/",
            "T_icon_ItemCategory_",
            ""),
    ];

    public static GameIconProbeResult Probe(
        SteamInstall install,
        string mappingPath,
        AssetContract sourceContract)
    {
        SecureInput.EnsureRegularFile(mappingPath);
        var mappingSha256 = Hashing.Sha256File(mappingPath);
        sourceContract.RequireProbeIdentity(install.BuildId, mappingSha256);
        try
        {
            var provider = SourceContractProbe.OpenProvider(
                install.InstallPath,
                mappingPath);
            var groups = Definitions
                .Select(definition =>
                {
                    var paths = SelectPackagePaths(
                        provider.Files.Keys,
                        definition);
                    return new GameIconGroupProbe(
                        definition.GroupId,
                        definition.AssetRoot,
                        definition.FileNamePrefix,
                        definition.FileNameSuffix,
                        paths.Count,
                        InventorySha256(paths),
                        paths);
                })
                .ToArray();
            return new GameIconProbeResult(
                install.BuildId,
                mappingSha256,
                groups);
        }
        catch (DataPackFailure)
        {
            throw;
        }
        catch (Exception error)
        {
            throw new DataPackFailure(
                DataPackExitCode.MountOrSerialization,
                $"game icon inventory failed ({error.GetType().Name})");
        }
    }

    public static IReadOnlyList<string> SelectPackagePaths(
        IEnumerable<string> filePaths,
        GameIconGroupDefinition definition) =>
        filePaths
            .Where(path => IsMatch(path, definition))
            .Select(path => path[..^".uasset".Length])
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .Order(StringComparer.OrdinalIgnoreCase)
            .ToArray();

    public static string LogicalId(
        string packagePath,
        GameIconGroupDefinition definition)
    {
        var fileName = Path.GetFileName(packagePath);
        if (!fileName.StartsWith(
                definition.FileNamePrefix,
                StringComparison.OrdinalIgnoreCase)
            || !fileName.EndsWith(
                definition.FileNameSuffix,
                StringComparison.OrdinalIgnoreCase)
            || fileName.Length
                <= definition.FileNamePrefix.Length
                    + definition.FileNameSuffix.Length)
        {
            throw new DataPackFailure(
                DataPackExitCode.SourceContractMismatch,
                $"game icon package does not match its group: {packagePath}");
        }
        return fileName[
            definition.FileNamePrefix.Length..(fileName.Length - definition.FileNameSuffix.Length)];
    }

    public static string InventorySha256(IReadOnlyList<string> packagePaths)
    {
        var canonical = string.Join('\n', packagePaths) + "\n";
        return Hashing.Sha256Hex(Encoding.UTF8.GetBytes(canonical));
    }

    private static bool IsMatch(
        string path,
        GameIconGroupDefinition definition)
    {
        if (!path.StartsWith(
                definition.AssetRoot,
                StringComparison.OrdinalIgnoreCase)
            || !path.EndsWith(".uasset", StringComparison.OrdinalIgnoreCase))
        {
            return false;
        }
        var fileName = Path.GetFileNameWithoutExtension(path);
        return fileName.StartsWith(
                definition.FileNamePrefix,
                StringComparison.OrdinalIgnoreCase)
            && fileName.EndsWith(
                definition.FileNameSuffix,
                StringComparison.OrdinalIgnoreCase)
            && fileName.Length
                > definition.FileNamePrefix.Length
                    + definition.FileNameSuffix.Length;
    }
}
