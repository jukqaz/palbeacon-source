using System.Text.RegularExpressions;

namespace PalMapPack;

public sealed record SteamInstall(string BuildId, string InstallPath, string ManifestPath);

public static partial class SteamInstallLocator
{
    public static SteamInstall Locate(string appId, IEnumerable<string> libraryRoots)
    {
        if (string.IsNullOrEmpty(appId) || appId.Any(character => !char.IsAsciiDigit(character)))
        {
            throw new ArgumentException("Steam App ID must contain only digits", nameof(appId));
        }

        foreach (var root in libraryRoots.Distinct(StringComparer.OrdinalIgnoreCase))
        {
            var manifestPath = Path.Combine(Path.GetFullPath(root), "steamapps", $"appmanifest_{appId}.acf");
            if (!File.Exists(manifestPath))
            {
                continue;
            }

            var text = File.ReadAllText(manifestPath);
            var build = Capture(text, "buildid");
            var installDirectory = Capture(text, "installdir");
            if (build is null || installDirectory is null || build.Any(character => !char.IsAsciiDigit(character)))
            {
                throw new MapPackFailure(ExitCodes.MountSerialization, "Steam appmanifest is missing a valid build identity");
            }

            var installPath = Path.Combine(Path.GetDirectoryName(manifestPath)!, "common", installDirectory);
            if (!Directory.Exists(installPath))
            {
                throw new MapPackFailure(ExitCodes.MountSerialization, "Steam appmanifest install directory is missing");
            }
            return new SteamInstall(build, Path.GetFullPath(installPath), Path.GetFullPath(manifestPath));
        }

        throw new MapPackFailure(ExitCodes.MountSerialization, "Steam App 1623730 was not found in supplied libraries");
    }

    public static SteamInstall LocateDefault(string appId)
    {
        var roots = new List<string>();
        var steamPath = Environment.GetEnvironmentVariable("PAL_STEAM_ROOT");
        if (!string.IsNullOrWhiteSpace(steamPath))
        {
            roots.Add(steamPath);
        }
        var programFiles = Environment.GetFolderPath(Environment.SpecialFolder.ProgramFilesX86);
        if (!string.IsNullOrWhiteSpace(programFiles))
        {
            roots.Add(Path.Combine(programFiles, "Steam"));
        }
        var libraries = roots.SelectMany(DiscoverLibraryRoots);
        return Locate(appId, libraries);
    }

    public static IReadOnlyList<string> DiscoverLibraryRoots(string steamRoot)
    {
        var root = Path.GetFullPath(steamRoot);
        var results = new List<string> { root };
        var file = Path.Combine(root, "steamapps", "libraryfolders.vdf");
        if (File.Exists(file))
        {
            var text = File.ReadAllText(file);
            foreach (Match match in AcfValueRegex().Matches(text))
            {
                if (match.Groups["key"].Value != "path")
                {
                    continue;
                }
                var candidate = match.Groups["value"].Value.Replace("\\\\", "\\", StringComparison.Ordinal);
                if (Path.IsPathFullyQualified(candidate))
                {
                    results.Add(Path.GetFullPath(candidate));
                }
            }
        }
        return results.Distinct(StringComparer.OrdinalIgnoreCase).ToArray();
    }

    private static string? Capture(string text, string key)
    {
        var match = AcfValueRegex().Matches(text)
            .Cast<Match>()
            .FirstOrDefault(match => string.Equals(match.Groups["key"].Value, key, StringComparison.Ordinal));
        return match?.Groups["value"].Value;
    }

    [GeneratedRegex("\"(?<key>[^\"]+)\"\\s*\"(?<value>[^\"]*)\"", RegexOptions.CultureInvariant)]
    private static partial Regex AcfValueRegex();
}
