using System.Text.RegularExpressions;

namespace PalDataPack;

public sealed record SteamInstall(
    string BuildId,
    string InstallPath,
    string ManifestPath);

public static partial class SteamInstallLocator
{
    public const string PalworldAppId = "1623730";

    public static SteamInstall LocateDefault()
    {
        var roots = new List<string>();
        var configured = Environment.GetEnvironmentVariable("PAL_STEAM_ROOT");
        if (!string.IsNullOrWhiteSpace(configured))
        {
            roots.Add(configured);
        }
        var programFiles = Environment.GetFolderPath(
            Environment.SpecialFolder.ProgramFilesX86);
        if (!string.IsNullOrWhiteSpace(programFiles))
        {
            roots.Add(Path.Combine(programFiles, "Steam"));
        }
        return Locate(roots.SelectMany(DiscoverLibraryRoots));
    }

    public static SteamInstall Locate(IEnumerable<string> libraryRoots)
    {
        foreach (var root in libraryRoots.Distinct(StringComparer.OrdinalIgnoreCase))
        {
            var manifest = Path.Combine(
                Path.GetFullPath(root),
                "steamapps",
                $"appmanifest_{PalworldAppId}.acf");
            if (!File.Exists(manifest))
            {
                continue;
            }
            var text = File.ReadAllText(manifest);
            var appId = Capture(text, "appid");
            var buildId = Capture(text, "buildid");
            var targetBuildId = Capture(text, "TargetBuildID");
            var installDirectory = Capture(text, "installdir");
            if (appId != PalworldAppId
                || string.IsNullOrEmpty(buildId)
                || buildId.Any(character => !char.IsAsciiDigit(character))
                || (!string.IsNullOrEmpty(targetBuildId) && targetBuildId != buildId)
                || string.IsNullOrWhiteSpace(installDirectory))
            {
                throw new DataPackFailure(
                    DataPackExitCode.SourceContractMismatch,
                    "Steam appmanifest has an inconsistent Build identity");
            }
            var installPath = Path.Combine(
                Path.GetDirectoryName(manifest)!,
                "common",
                installDirectory);
            SecureInput.EnsurePlainDirectory(installPath);
            return new SteamInstall(
                buildId,
                Path.GetFullPath(installPath),
                Path.GetFullPath(manifest));
        }
        throw new DataPackFailure(
            DataPackExitCode.SourceContractMismatch,
            "Palworld Steam installation was not found");
    }

    public static IReadOnlyList<string> DiscoverLibraryRoots(string steamRoot)
    {
        var root = Path.GetFullPath(steamRoot);
        var results = new List<string> { root };
        var libraries = Path.Combine(root, "steamapps", "libraryfolders.vdf");
        if (File.Exists(libraries))
        {
            foreach (Match match in AcfValueRegex().Matches(File.ReadAllText(libraries)))
            {
                if (match.Groups["key"].Value == "path")
                {
                    var candidate = match.Groups["value"].Value.Replace(
                        "\\\\",
                        "\\",
                        StringComparison.Ordinal);
                    if (Path.IsPathFullyQualified(candidate))
                    {
                        results.Add(Path.GetFullPath(candidate));
                    }
                }
            }
        }
        return results.Distinct(StringComparer.OrdinalIgnoreCase).ToArray();
    }

    private static string? Capture(string text, string key) =>
        AcfValueRegex().Matches(text)
            .Cast<Match>()
            .FirstOrDefault(match =>
                string.Equals(match.Groups["key"].Value, key, StringComparison.Ordinal))
            ?.Groups["value"].Value;

    [GeneratedRegex(
        "\"(?<key>[^\"]+)\"\\s*\"(?<value>[^\"]*)\"",
        RegexOptions.CultureInvariant)]
    private static partial Regex AcfValueRegex();
}
