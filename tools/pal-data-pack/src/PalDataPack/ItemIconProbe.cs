using System.Text.Json;
using System.Text.RegularExpressions;

namespace PalDataPack;

public sealed record ItemIconMatch(
    string IconName,
    string PackagePath,
    string MatchKind,
    int ReferencingItemCount,
    bool LegalInGame);

public sealed record ItemIconProbeResult(
    string GameBuildId,
    string MappingSha256,
    string ItemCatalogSha256,
    string AssetRoot,
    int ItemCount,
    int RequestedIconCount,
    int LegalRequestedIconCount,
    int InventoryTextureCount,
    int ExactMatchCount,
    int TypedExactMatchCount,
    int CategorySuffixMatchCount,
    int SharedVariantMatchCount,
    int ReviewedAliasMatchCount,
    int LegalMatchCount,
    IReadOnlyList<ItemIconMatch> Matches,
    IReadOnlyList<string> MissingIconNames,
    IReadOnlyList<string> LegalMissingIconNames,
    IReadOnlyDictionary<string, IReadOnlyList<string>> AmbiguousIconNames,
    IReadOnlyDictionary<string, IReadOnlyList<string>> LegalAmbiguousIconNames);

public static partial class ItemIconProbe
{
    public const string AssetRoot =
        "Pal/Content/Others/InventoryItemIcon/Texture/";
    public static readonly IReadOnlyList<string> TexturePrefixes =
    [
        "T_itemicon_",
        "T_icon_item_",
    ];
    private static readonly IReadOnlyDictionary<string, string> ReviewedAliases =
        new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
        {
            ["AssaultRifle_Default"] = "Weapon_AssaultRifle_Default1",
            ["Axe_Default"] = "Weapon_Axe_Tier_00",
            ["Axe_Steal"] = "Weapon_Axe_Tier_03",
            ["CapturePrism"] = "PalSphere",
            ["FlameThrower"] = "Weapon_FlameThrower_Default",
            ["LeatherClothing"] = "Armor_ClothArmor",
            ["PalEgg_Normal_01"] = "Material_PalEgg",
            ["PickAxe_Default"] = "Weapon_Pickaxe_Tier_00",
            ["Pickaxe_Steal"] = "Weapon_Pickaxe_Tier_03",
            ["Shield_SF"] = "Armor_Shield_06",
            ["ShotGun_Default"] = "Weapon_PumpActionShotgun",
            ["Spear"] = "Weapon_Spear_Tier_00",
            ["SphereModule_Sniper"] = "SphereModule_Sniper2",
            ["StirFriedVegetables"] = "Food_FriedVegetables",
        };

    private const int MaximumItemCatalogBytes = 32 * 1024 * 1024;

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        UnmappedMemberHandling = System.Text.Json.Serialization.JsonUnmappedMemberHandling.Disallow,
        MaxDepth = 64,
    };

    [GeneratedRegex(@"\d+$", RegexOptions.CultureInvariant)]
    private static partial Regex TrailingDigits();

    public static ItemIconProbeResult Probe(
        SteamInstall install,
        string mappingPath,
        AssetContract itemContract,
        string itemCatalogPath)
    {
        SecureInput.EnsureRegularFile(mappingPath);
        var mappingSha256 = Hashing.Sha256File(mappingPath);
        itemContract.RequireExtractionIdentity(install.BuildId, mappingSha256);
        var itemCatalogBytes = SecureInput.ReadBoundedRegularFile(
            itemCatalogPath,
            MaximumItemCatalogBytes);
        var itemCatalogSha256 = Hashing.Sha256Hex(itemCatalogBytes);
        if (itemContract.ExpectedOutputSha256 is null
            || !string.Equals(
                itemCatalogSha256,
                itemContract.ExpectedOutputSha256,
                StringComparison.Ordinal))
        {
            throw new DataPackFailure(
                DataPackExitCode.AssetContractMismatch,
                "item icon probe requires the reviewed exact-Build item catalog");
        }
        var catalog = JsonSerializer.Deserialize<VerifiedItemCatalogDocument>(
                itemCatalogBytes,
                JsonOptions)
            ?? throw new DataPackFailure(
                DataPackExitCode.SourceContractMismatch,
                "item catalog is empty");
        if (!catalog.Verified
            || !string.Equals(
                catalog.GameBuildId,
                $"steam:{install.BuildId}",
                StringComparison.Ordinal)
            || !string.Equals(
                catalog.MappingSha256,
                mappingSha256,
                StringComparison.Ordinal))
        {
            throw new DataPackFailure(
                DataPackExitCode.AssetContractMismatch,
                "item catalog identity does not match the icon probe inputs");
        }

        try
        {
            var provider = SourceContractProbe.OpenProvider(
                install.InstallPath,
                mappingPath);
            var inventory = provider.Files.Keys
                .Where(path =>
                    path.StartsWith(AssetRoot, StringComparison.OrdinalIgnoreCase)
                    && path.EndsWith(".uasset", StringComparison.OrdinalIgnoreCase)
                    && TexturePrefixes.Any(prefix =>
                        Path.GetFileName(path).StartsWith(
                            prefix,
                            StringComparison.OrdinalIgnoreCase)))
                .Select(path => path[..^".uasset".Length])
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .Order(StringComparer.OrdinalIgnoreCase)
                .ToArray();
            var assetsByName = inventory
                .GroupBy(
                    path => LogicalIconName(path),
                    StringComparer.OrdinalIgnoreCase)
                .ToDictionary(
                    group => group.Key,
                    group => group.ToArray(),
                    StringComparer.OrdinalIgnoreCase);
            var requests = catalog.Items
                .Where(item => !string.Equals(item.IconName, "-", StringComparison.Ordinal))
                .GroupBy(item => item.IconName, StringComparer.OrdinalIgnoreCase)
                .OrderBy(group => group.Key, StringComparer.OrdinalIgnoreCase)
                .ToArray();

            var matches = new List<ItemIconMatch>(requests.Length);
            var missing = new List<string>();
            var ambiguous = new SortedDictionary<string, IReadOnlyList<string>>(
                StringComparer.OrdinalIgnoreCase);
            var legalMissing = new List<string>();
            var legalAmbiguous = new SortedDictionary<string, IReadOnlyList<string>>(
                StringComparer.OrdinalIgnoreCase);
            foreach (var request in requests)
            {
                var legalInGame = request.Any(item => item.LegalInGame);
                var itemTypes = request
                    .Select(item => item.TypeA)
                    .Where(type => !string.IsNullOrWhiteSpace(type))
                    .Distinct(StringComparer.OrdinalIgnoreCase)
                    .ToArray();
                var resolution = Resolve(
                    request.Key,
                    assetsByName,
                    itemTypes.Length == 1 ? itemTypes[0] : null);
                if (resolution.PackagePaths.Count == 0)
                {
                    missing.Add(request.Key);
                    if (legalInGame)
                    {
                        legalMissing.Add(request.Key);
                    }
                }
                else if (resolution.PackagePaths.Count > 1)
                {
                    ambiguous.Add(request.Key, resolution.PackagePaths);
                    if (legalInGame)
                    {
                        legalAmbiguous.Add(request.Key, resolution.PackagePaths);
                    }
                }
                else
                {
                    matches.Add(new ItemIconMatch(
                        request.Key,
                        resolution.PackagePaths[0],
                        resolution.MatchKind,
                        request.Count(),
                        legalInGame));
                }
            }
            return new ItemIconProbeResult(
                install.BuildId,
                mappingSha256,
                itemCatalogSha256,
                AssetRoot,
                catalog.Items.Count,
                requests.Length,
                requests.Count(request => request.Any(item => item.LegalInGame)),
                inventory.Length,
                matches.Count(match => match.MatchKind == "exact"),
                matches.Count(match => match.MatchKind == "typed_exact"),
                matches.Count(match => match.MatchKind == "category_suffix"),
                matches.Count(match => match.MatchKind == "shared_variant"),
                matches.Count(match => match.MatchKind == "reviewed_alias"),
                matches.Count(match => match.LegalInGame),
                matches,
                missing,
                legalMissing,
                ambiguous,
                legalAmbiguous);
        }
        catch (DataPackFailure)
        {
            throw;
        }
        catch (Exception error)
        {
            throw new DataPackFailure(
                DataPackExitCode.MountOrSerialization,
                $"item icon inventory failed ({error.GetType().Name})");
        }
    }

    public static ItemIconResolution Resolve(
        string iconName,
        IReadOnlyDictionary<string, string[]> assetsByName,
        string? itemType = null)
    {
        if (assetsByName.TryGetValue(iconName, out var exact))
        {
            return new ItemIconResolution("exact", exact);
        }
        if (!string.IsNullOrWhiteSpace(itemType)
            && assetsByName.TryGetValue(
                $"{itemType}_{iconName}",
                out var typedExact))
        {
            return new ItemIconResolution("typed_exact", typedExact);
        }
        if (ReviewedAliases.TryGetValue(iconName, out var reviewedAlias)
            && assetsByName.TryGetValue(reviewedAlias, out var reviewed))
        {
            return new ItemIconResolution("reviewed_alias", reviewed);
        }
        var categorySuffix = SuffixMatches(iconName, assetsByName);
        if (categorySuffix.Count > 0)
        {
            return new ItemIconResolution("category_suffix", categorySuffix);
        }
        var withoutDigits = TrailingDigits().Replace(iconName, string.Empty);
        var candidateNames = new[]
            {
                withoutDigits,
                $"{iconName}_1",
                $"{withoutDigits}_1",
            }
            .Where(candidate => !string.Equals(
                candidate,
                iconName,
                StringComparison.OrdinalIgnoreCase))
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToArray();
        var candidates = candidateNames
            .SelectMany(candidate =>
            {
                var matches = new List<string>();
                if (assetsByName.TryGetValue(candidate, out var direct))
                {
                    matches.AddRange(direct);
                }
                matches.AddRange(SuffixMatches(candidate, assetsByName));
                return matches;
            })
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .Order(StringComparer.OrdinalIgnoreCase)
            .ToArray();
        return new ItemIconResolution(
            candidates.Length == 0 ? "missing" : "shared_variant",
            candidates);
    }

    private static IReadOnlyList<string> SuffixMatches(
        string iconName,
        IReadOnlyDictionary<string, string[]> assetsByName)
    {
        var suffix = $"_{iconName}";
        return assetsByName
            .Where(pair => pair.Key.EndsWith(
                suffix,
                StringComparison.OrdinalIgnoreCase))
            .SelectMany(pair => pair.Value)
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .Order(StringComparer.OrdinalIgnoreCase)
            .ToArray();
    }

    private static string LogicalIconName(string packagePath)
    {
        var fileName = Path.GetFileName(packagePath);
        var prefix = TexturePrefixes.FirstOrDefault(candidate =>
            fileName.StartsWith(candidate, StringComparison.OrdinalIgnoreCase));
        if (prefix is null)
        {
            throw new DataPackFailure(
                DataPackExitCode.SourceContractMismatch,
                $"unsupported item icon package name: {fileName}");
        }
        return fileName[prefix.Length..];
    }
}

public sealed record ItemIconResolution(
    string MatchKind,
    IReadOnlyList<string> PackagePaths);
