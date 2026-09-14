using System.Text.Json;

namespace PalDataPack;

public static class Program
{
    public static async Task<int> Main(string[] args)
    {
        try
        {
            if (args.Length == 0)
            {
                throw Usage();
            }
            var command = args[0];
            var options = ParseOptions(args[1..]);
            switch (command)
            {
                case "probe":
                    return Probe(options);
                case "discover-paths":
                    return DiscoverPaths(options);
                case "sample-table":
                    return SampleTable(options);
                case "sample-package":
                    return SamplePackage(options);
                case "extract-items":
                    return ExtractItems(options);
                case "extract-pal-breeding":
                    return ExtractPalBreeding(options);
                case "extract-humans":
                    return ExtractHumans(options);
                case "build-pal-matches":
                    return BuildPalMatches(options);
                case "build-pal-relations":
                    return BuildPalRelations(options);
                case "extract-world":
                    return ExtractWorld(options);
                case "extract-map-spawns":
                    return ExtractMapSpawns(options);
                case "index-map-spawns":
                    return IndexMapSpawns(options);
                case "extract-korean-ui":
                    return ExtractKoreanUi(options);
                case "extract-korean-catalog":
                    return ExtractKoreanCatalog(options);
                case "probe-item-icons":
                    return ProbeItemIcons(options);
                case "extract-item-icons":
                    return ExtractItemIcons(options);
                case "extract-building-icons":
                    return ExtractBuildingIcons(options);
                case "probe-game-icons":
                    return ProbeGameIcons(options);
                case "extract-game-icons":
                    return ExtractGameIcons(options);
                case "extract-pal-icon-links":
                    return ExtractPalIconLinks(options);
                case "match-game-icons":
                    return MatchGameIcons(options);
                case "publish-normalized":
                    return await PublishNormalizedAsync(options);
                case "validate-package":
                    return ValidatePackage(options);
                default:
                    throw Usage();
            }
        }
        catch (DataPackFailure error)
        {
            Console.Error.WriteLine(error.Message);
            return (int)error.ExitCode;
        }
        catch (Exception error)
        {
            Console.Error.WriteLine(error.Message);
            return (int)DataPackExitCode.MountOrSerialization;
        }
    }

    private static int Probe(IReadOnlyDictionary<string, string> options)
    {
        var install = Required(options, "install") == "auto"
            ? SteamInstallLocator.LocateDefault()
            : InstallFromExplicitPath(
                Required(options, "install"),
                Required(options, "build"));
        var contract = AssetContract.Load(Required(options, "contract"));
        var result = SourceContractProbe.Probe(
            install,
            Required(options, "mapping"),
            contract);
        Console.WriteLine(JsonSerializer.Serialize(
            result,
            new JsonSerializerOptions
            {
                WriteIndented = true,
                PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
            }));
        return (int)DataPackExitCode.Success;
    }

    private static int DiscoverPaths(IReadOnlyDictionary<string, string> options)
    {
        var install = Required(options, "install") == "auto"
            ? SteamInstallLocator.LocateDefault()
            : InstallFromExplicitPath(
                Required(options, "install"),
                Required(options, "build"));
        var contract = AssetContract.Load(Required(options, "contract"));
        var result = SourceContractProbe.DiscoverPaths(
            install,
            Required(options, "mapping"),
            contract,
            Required(options, "contains"));
        Console.WriteLine(JsonSerializer.Serialize(
            result,
            new JsonSerializerOptions
            {
                WriteIndented = true,
                PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
            }));
        return (int)DataPackExitCode.Success;
    }

    private static int SampleTable(IReadOnlyDictionary<string, string> options)
    {
        var install = Required(options, "install") == "auto"
            ? SteamInstallLocator.LocateDefault()
            : InstallFromExplicitPath(
                Required(options, "install"),
                Required(options, "build"));
        var contract = AssetContract.Load(Required(options, "contract"));
        var result = SourceContractProbe.SampleTable(
            install,
            Required(options, "mapping"),
            contract,
            Required(options, "package-path"),
            options.TryGetValue("limit", out var rawLimit)
                && int.TryParse(rawLimit, out var limit)
                    ? limit
                    : 5,
            options.TryGetValue("row-key", out var rowKey) ? rowKey : null,
            options.TryGetValue("row-contains", out var rowContains)
                ? rowContains
                : null,
            options.TryGetValue("text-contains", out var textContains)
                ? textContains
                : null);
        Console.WriteLine(JsonSerializer.Serialize(
            result,
            new JsonSerializerOptions
            {
                WriteIndented = true,
                PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
            }));
        return (int)DataPackExitCode.Success;
    }

    private static int SamplePackage(
        IReadOnlyDictionary<string, string> options)
    {
        var install = Required(options, "install") == "auto"
            ? SteamInstallLocator.LocateDefault()
            : InstallFromExplicitPath(
                Required(options, "install"),
                Required(options, "build"));
        var contract = AssetContract.Load(Required(options, "contract"));
        var result = PackageSampleProbe.Probe(
            install,
            Required(options, "mapping"),
            contract,
            Required(options, "package-path"));
        Console.WriteLine(JsonSerializer.Serialize(
            result,
            new JsonSerializerOptions
            {
                WriteIndented = true,
                PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
            }));
        return (int)DataPackExitCode.Success;
    }

    private static int ExtractItems(IReadOnlyDictionary<string, string> options)
    {
        var install = Required(options, "install") == "auto"
            ? SteamInstallLocator.LocateDefault()
            : InstallFromExplicitPath(
                Required(options, "install"),
                Required(options, "build"));
        var contract = AssetContract.Load(Required(options, "contract"));
        var result = VerifiedItemCatalogExtractor.ExtractToFile(
            install,
            Required(options, "mapping"),
            contract,
            Required(options, "output"));
        Console.WriteLine(JsonSerializer.Serialize(
            result,
            new JsonSerializerOptions
            {
                WriteIndented = true,
                PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
            }));
        return (int)DataPackExitCode.Success;
    }

    private static int ExtractPalBreeding(
        IReadOnlyDictionary<string, string> options)
    {
        var install = Required(options, "install") == "auto"
            ? SteamInstallLocator.LocateDefault()
            : InstallFromExplicitPath(
                Required(options, "install"),
                Required(options, "build"));
        var contract = AssetContract.Load(Required(options, "contract"));
        var result = VerifiedPalBreedingExtractor.ExtractToFile(
            install,
            Required(options, "mapping"),
            contract,
            Required(options, "output"));
        Console.WriteLine(JsonSerializer.Serialize(
            result,
            new JsonSerializerOptions
            {
                WriteIndented = true,
                PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
            }));
        return (int)DataPackExitCode.Success;
    }

    private static int ExtractHumans(
        IReadOnlyDictionary<string, string> options)
    {
        var install = Required(options, "install") == "auto"
            ? SteamInstallLocator.LocateDefault()
            : InstallFromExplicitPath(
                Required(options, "install"),
                Required(options, "build"));
        var contract = AssetContract.Load(Required(options, "contract"));
        var result = VerifiedHumanCatalogExtractor.ExtractToFile(
            install,
            Required(options, "mapping"),
            contract,
            Required(options, "output"));
        Console.WriteLine(JsonSerializer.Serialize(
            result,
            new JsonSerializerOptions
            {
                WriteIndented = true,
                PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
            }));
        return (int)DataPackExitCode.Success;
    }

    private static int BuildPalMatches(
        IReadOnlyDictionary<string, string> options)
    {
        var result = PalCanonicalMatchBuilder.BuildToFile(
            Required(options, "pal-catalog"),
            Required(options, "icon-matches"),
            Required(options, "pal-icon-links"),
            Required(options, "output"));
        Console.WriteLine(JsonSerializer.Serialize(
            result,
            new JsonSerializerOptions
            {
                WriteIndented = true,
                PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
            }));
        return (int)DataPackExitCode.Success;
    }

    private static int BuildPalRelations(
        IReadOnlyDictionary<string, string> options)
    {
        var result = PalRelationMatchBuilder.BuildToFile(
            Required(options, "pal-catalog"),
            Required(options, "human-catalog"),
            Required(options, "item-catalog"),
            Required(options, "spawn-search"),
            Required(options, "output"));
        Console.WriteLine(JsonSerializer.Serialize(
            result,
            new JsonSerializerOptions
            {
                WriteIndented = true,
                PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
            }));
        return (int)DataPackExitCode.Success;
    }

    private static int ExtractWorld(IReadOnlyDictionary<string, string> options)
    {
        var install = Required(options, "install") == "auto"
            ? SteamInstallLocator.LocateDefault()
            : InstallFromExplicitPath(
                Required(options, "install"),
                Required(options, "build"));
        var contract = AssetContract.Load(Required(options, "contract"));
        var result = VerifiedWorldCatalogExtractor.ExtractToFile(
            install,
            Required(options, "mapping"),
            contract,
            Required(options, "item-catalog"),
            Required(options, "output"));
        Console.WriteLine(JsonSerializer.Serialize(
            result,
            new JsonSerializerOptions
            {
                WriteIndented = true,
                PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
            }));
        return (int)DataPackExitCode.Success;
    }

    private static int ExtractMapSpawns(IReadOnlyDictionary<string, string> options)
    {
        var install = Required(options, "install") == "auto"
            ? SteamInstallLocator.LocateDefault()
            : InstallFromExplicitPath(
                Required(options, "install"),
                Required(options, "build"));
        var contract = AssetContract.Load(Required(options, "contract"));
        var result = VerifiedMapSpawnExtractor.ExtractToFile(
            install,
            Required(options, "mapping"),
            contract,
            Required(options, "output"));
        Console.WriteLine(JsonSerializer.Serialize(
            result,
            new JsonSerializerOptions
            {
                WriteIndented = true,
                PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
            }));
        return (int)DataPackExitCode.Success;
    }

    private static int IndexMapSpawns(IReadOnlyDictionary<string, string> options)
    {
        var result = MapSpawnSearchIndexBuilder.BuildToFile(
            Required(options, "source"),
            Required(options, "output"));
        Console.WriteLine(JsonSerializer.Serialize(
            result,
            new JsonSerializerOptions
            {
                WriteIndented = true,
                PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
            }));
        return (int)DataPackExitCode.Success;
    }

    private static int ExtractKoreanUi(IReadOnlyDictionary<string, string> options)
    {
        var install = Required(options, "install") == "auto"
            ? SteamInstallLocator.LocateDefault()
            : InstallFromExplicitPath(
                Required(options, "install"),
                Required(options, "build"));
        var contract = AssetContract.Load(Required(options, "contract"));
        var result = VerifiedKoreanUiExtractor.ExtractToFile(
            install,
            Required(options, "mapping"),
            contract,
            Required(options, "output"));
        Console.WriteLine(JsonSerializer.Serialize(
            result,
            new JsonSerializerOptions
            {
                WriteIndented = true,
                PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
            }));
        return (int)DataPackExitCode.Success;
    }

    private static int ExtractKoreanCatalog(IReadOnlyDictionary<string, string> options)
    {
        var install = Required(options, "install") == "auto"
            ? SteamInstallLocator.LocateDefault()
            : InstallFromExplicitPath(
                Required(options, "install"),
                Required(options, "build"));
        var contract = AssetContract.Load(Required(options, "contract"));
        var result = VerifiedKoreanCatalogExtractor.ExtractToDirectory(
            install,
            Required(options, "mapping"),
            contract,
            Required(options, "output"));
        Console.WriteLine(JsonSerializer.Serialize(
            result,
            new JsonSerializerOptions
            {
                WriteIndented = true,
                PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
            }));
        return (int)DataPackExitCode.Success;
    }

    private static int ProbeItemIcons(IReadOnlyDictionary<string, string> options)
    {
        var install = Required(options, "install") == "auto"
            ? SteamInstallLocator.LocateDefault()
            : InstallFromExplicitPath(
                Required(options, "install"),
                Required(options, "build"));
        var contract = AssetContract.Load(Required(options, "contract"));
        var result = ItemIconProbe.Probe(
            install,
            Required(options, "mapping"),
            contract,
            Required(options, "item-catalog"));
        Console.WriteLine(JsonSerializer.Serialize(
            result,
            new JsonSerializerOptions
            {
                WriteIndented = true,
                PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
            }));
        return (int)DataPackExitCode.Success;
    }

    private static int ExtractItemIcons(IReadOnlyDictionary<string, string> options)
    {
        var install = Required(options, "install") == "auto"
            ? SteamInstallLocator.LocateDefault()
            : InstallFromExplicitPath(
                Required(options, "install"),
                Required(options, "build"));
        var itemCatalogContract = AssetContract.Load(Required(options, "contract"));
        var iconContract = ItemIconContract.Load(Required(options, "icon-contract"));
        var result = ItemIconExtractor.Extract(
            install,
            Required(options, "mapping"),
            itemCatalogContract,
            Required(options, "item-catalog"),
            iconContract,
            Required(options, "output"));
        Console.WriteLine(JsonSerializer.Serialize(
            result,
            new JsonSerializerOptions
            {
                WriteIndented = true,
                PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
            }));
        return (int)DataPackExitCode.Success;
    }

    private static int ExtractBuildingIcons(IReadOnlyDictionary<string, string> options)
    {
        var install = Required(options, "install") == "auto"
            ? SteamInstallLocator.LocateDefault()
            : InstallFromExplicitPath(
                Required(options, "install"),
                Required(options, "build"));
        var worldContract = AssetContract.Load(Required(options, "contract"));
        var result = BuildingIconExtractor.Extract(
            install,
            Required(options, "mapping"),
            worldContract,
            Required(options, "world-catalog"),
            Required(options, "output"));
        Console.WriteLine(JsonSerializer.Serialize(
            result,
            new JsonSerializerOptions
            {
                WriteIndented = true,
                PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
            }));
        return (int)DataPackExitCode.Success;
    }

    private static int ProbeGameIcons(
        IReadOnlyDictionary<string, string> options)
    {
        var install = Required(options, "install") == "auto"
            ? SteamInstallLocator.LocateDefault()
            : InstallFromExplicitPath(
                Required(options, "install"),
                Required(options, "build"));
        var sourceContract = AssetContract.Load(Required(options, "contract"));
        var result = GameIconProbe.Probe(
            install,
            Required(options, "mapping"),
            sourceContract);
        Console.WriteLine(JsonSerializer.Serialize(
            result,
            new JsonSerializerOptions
            {
                WriteIndented = true,
                PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
            }));
        return (int)DataPackExitCode.Success;
    }

    private static int ExtractGameIcons(
        IReadOnlyDictionary<string, string> options)
    {
        var install = Required(options, "install") == "auto"
            ? SteamInstallLocator.LocateDefault()
            : InstallFromExplicitPath(
                Required(options, "install"),
                Required(options, "build"));
        var sourceContract = AssetContract.Load(Required(options, "contract"));
        var iconContract = GameIconContract.Load(
            Required(options, "icon-contract"));
        var result = GameIconExtractor.Extract(
            install,
            Required(options, "mapping"),
            sourceContract,
            iconContract,
            Required(options, "output"));
        Console.WriteLine(JsonSerializer.Serialize(
            result,
            new JsonSerializerOptions
            {
                WriteIndented = true,
                PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
            }));
        return (int)DataPackExitCode.Success;
    }

    private static int MatchGameIcons(
        IReadOnlyDictionary<string, string> options)
    {
        var iconContract = GameIconContract.Load(
            Required(options, "icon-contract"));
        var result = GameIconLocalizationMatcher.BuildToFile(
            Required(options, "icon-manifest"),
            Required(options, "korean-pal-catalog"),
            Required(options, "pal-icon-links"),
            iconContract,
            Required(options, "output"));
        Console.WriteLine(JsonSerializer.Serialize(
            result,
            new JsonSerializerOptions
            {
                WriteIndented = true,
                PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
            }));
        return (int)DataPackExitCode.Success;
    }

    private static int ExtractPalIconLinks(
        IReadOnlyDictionary<string, string> options)
    {
        var install = Required(options, "install") == "auto"
            ? SteamInstallLocator.LocateDefault()
            : InstallFromExplicitPath(
                Required(options, "install"),
                Required(options, "build"));
        var contract = AssetContract.Load(Required(options, "contract"));
        var result = PalIconSourceLinkExtractor.ExtractToFile(
            install,
            Required(options, "mapping"),
            contract,
            Required(options, "output"));
        Console.WriteLine(JsonSerializer.Serialize(
            result,
            new JsonSerializerOptions
            {
                WriteIndented = true,
                PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
            }));
        return (int)DataPackExitCode.Success;
    }

    private static async Task<int> PublishNormalizedAsync(
        IReadOnlyDictionary<string, string> options)
    {
        var publication = await FixturePipeline.PublishAsync(
            Required(options, "source"),
            Required(options, "dataset-root"),
            Required(options, "build"),
            CancellationToken.None);
        Console.WriteLine(JsonSerializer.Serialize(
            new
            {
                game_build_id = publication.Manifest.GameBuildId,
                dataset_manifest_id = publication.Manifest.DatasetManifestId,
                version_path = publication.VersionPath,
                active_pointer_path = publication.ActivePointerPath,
            },
            new JsonSerializerOptions { WriteIndented = true }));
        return (int)DataPackExitCode.Success;
    }

    private static int ValidatePackage(IReadOnlyDictionary<string, string> options)
    {
        var manifest = PackageValidator.ReopenAndVerify(Required(options, "path"));
        Console.WriteLine(JsonSerializer.Serialize(
            new
            {
                valid = true,
                game_build_id = manifest.GameBuildId,
                dataset_manifest_id = manifest.DatasetManifestId,
                capability_count = manifest.Capabilities.Count,
                file_count = manifest.Files.Count,
            },
            new JsonSerializerOptions { WriteIndented = true }));
        return (int)DataPackExitCode.Success;
    }

    private static SteamInstall InstallFromExplicitPath(
        string installPath,
        string buildId)
    {
        if (string.IsNullOrEmpty(buildId)
            || buildId.Any(character => !char.IsAsciiDigit(character)))
        {
            throw Usage();
        }
        SecureInput.EnsurePlainDirectory(installPath);
        return new SteamInstall(buildId, Path.GetFullPath(installPath), "explicit");
    }

    private static IReadOnlyDictionary<string, string> ParseOptions(string[] args)
    {
        if (args.Length % 2 != 0)
        {
            throw Usage();
        }
        var options = new Dictionary<string, string>(StringComparer.Ordinal);
        for (var index = 0; index < args.Length; index += 2)
        {
            var key = args[index];
            if (!key.StartsWith("--", StringComparison.Ordinal)
                || key.Length <= 2
                || !options.TryAdd(key[2..], args[index + 1]))
            {
                throw Usage();
            }
        }
        return options;
    }

    private static string Required(
        IReadOnlyDictionary<string, string> options,
        string name) =>
        options.TryGetValue(name, out var value) && !string.IsNullOrWhiteSpace(value)
            ? value
            : throw Usage();

    private static DataPackFailure Usage() => new(
        DataPackExitCode.Usage,
        "usage: PalDataPack probe --install auto|<path> [--build <id>] "
        + "--mapping <Mappings.usmap> --contract <contract.json> | "
        + "discover-paths --install auto|<path> [--build <id>] "
        + "--mapping <Mappings.usmap> --contract <contract.json> --contains <token> | "
        + "sample-table --install auto|<path> [--build <id>] "
        + "--mapping <Mappings.usmap> --contract <contract.json> "
        + "--package-path <contract-table-path> [--limit <1..20>] "
        + "[--row-key <exact-row-key> | --row-contains <row-key-token> "
        + "| --text-contains <localized-text-token>] | "
        + "sample-package --install auto|<path> [--build <id>] "
        + "--mapping <Mappings.usmap> --contract <exact-build-contract.json> "
        + "--package-path <reviewed-item-rarity-widget-path> | "
        + "extract-items --install auto|<path> [--build <id>] "
        + "--mapping <Mappings.usmap> --contract <reviewed-item-contract.json> "
        + "--output <items.v1.json> | "
        + "extract-pal-breeding --install auto|<path> [--build <id>] "
        + "--mapping <Mappings.usmap> "
        + "--contract <reviewed-pal-breeding-contract.json> "
        + "--output <pals-breeding.v1.json> | "
        + "extract-humans --install auto|<path> [--build <id>] "
        + "--mapping <Mappings.usmap> "
        + "--contract <reviewed-human-relation-contract.json> "
        + "--output <humans.v1.json> | "
        + "build-pal-matches --pal-catalog <pals-breeding.v1.json> "
        + "--icon-matches <game-icon-match-report.json> "
        + "--pal-icon-links <pal-icon-links.json> "
        + "--output <pal-canonical-matches.json> | "
        + "build-pal-relations --pal-catalog <pals-breeding.v1.json> "
        + "--human-catalog <humans.v1.json> "
        + "--item-catalog <items.v1.json> "
        + "--spawn-search <spawns.search.v1.json> "
        + "--output <pal-relation-matches.json> | "
        + "extract-world --install auto|<path> [--build <id>] "
        + "--mapping <Mappings.usmap> --contract <reviewed-world-contract.json> "
        + "--item-catalog <items.v1.json> --output <world.v1.json> | "
        + "extract-map-spawns --install auto|<path> [--build <id>] "
        + "--mapping <Mappings.usmap> --contract <reviewed-map-spawn-contract.json> "
        + "--output <map-spawns.v1.json> | "
        + "extract-korean-ui --install auto|<path> [--build <id>] "
        + "--mapping <Mappings.usmap> --contract <reviewed-korean-ui-contract.json> "
        + "--output <korean-ui.v1.json> | "
        + "extract-korean-catalog --install auto|<path> [--build <id>] "
        + "--mapping <Mappings.usmap> --contract <reviewed-korean-catalog-contract.json> "
        + "--output <directory> | "
        + "probe-item-icons --install auto|<path> [--build <id>] "
        + "--mapping <Mappings.usmap> --contract <reviewed-item-contract.json> "
        + "--item-catalog <items.v1.json> | "
        + "extract-item-icons --install auto|<path> [--build <id>] "
        + "--mapping <Mappings.usmap> --contract <reviewed-item-contract.json> "
        + "--item-catalog <items.v1.json> --icon-contract <reviewed-icon-contract.json> "
        + "--output <directory> | "
        + "extract-building-icons --install auto|<path> [--build <id>] "
        + "--mapping <Mappings.usmap> --contract <reviewed-world-contract.json> "
        + "--world-catalog <world.v1.json> --output <directory> | "
        + "probe-game-icons --install auto|<path> [--build <id>] "
        + "--mapping <Mappings.usmap> --contract <exact-build-contract.json> | "
        + "extract-game-icons --install auto|<path> [--build <id>] "
        + "--mapping <Mappings.usmap> --contract <exact-build-contract.json> "
        + "--icon-contract <reviewed-game-icon-contract.json> "
        + "--output <local-only-directory> | "
        + "extract-pal-icon-links --install auto|<path> [--build <id>] "
        + "--mapping <Mappings.usmap> "
        + "--contract <reviewed-pal-icon-link-contract.json> "
        + "--output <pal-icon-links.json> | "
        + "match-game-icons --icon-manifest <local-manifest.json> "
        + "--korean-pal-catalog <verified-ko-pals.json> "
        + "--pal-icon-links <exact-build-pal-icon-links.json> "
        + "--icon-contract <reviewed-game-icon-contract.json> "
        + "--output <match-report.json> | "
        + "publish-normalized --source <directory> --dataset-root <directory> "
        + "--build <steam:BuildID> | validate-package --path <version-directory>");
}
