namespace PalDataPack;

public static class CatalogContract
{
    public const int SchemaMajor = 1;
    public const int MaximumFiles = 32;
    public const long MaximumFileBytes = 256L * 1024 * 1024;
    public const int MaximumLineBytes = 1024 * 1024;
    public const int MaximumIdentifierBytes = 128;

    public static readonly IReadOnlyList<string> RequiredCapabilities =
    [
        "pals",
        "skills",
        "items",
        "recipes",
        "acquisition",
        "breeding",
        "technologies",
        "buildings",
        "locations",
        "pois",
        "aliases",
        "localization",
        "sources",
    ];

    public static readonly IReadOnlyDictionary<string, string> CapabilityFiles =
        new Dictionary<string, string>(StringComparer.Ordinal)
        {
            ["pals"] = "pals.ndjson",
            ["skills"] = "skills.ndjson",
            ["items"] = "items.ndjson",
            ["recipes"] = "recipes.ndjson",
            ["acquisition"] = "acquisition.ndjson",
            ["breeding"] = "breeding.ndjson",
            ["technologies"] = "world.ndjson",
            ["buildings"] = "world.ndjson",
            ["locations"] = "world.ndjson",
            ["pois"] = "world.ndjson",
            ["aliases"] = "world.ndjson",
            ["localization"] = "localization.ndjson",
            ["sources"] = "sources.ndjson",
        };

    public static string SafeBuildFileName(string gameBuildId)
    {
        RequireBuildId(gameBuildId);
        return gameBuildId.Replace(':', '_');
    }

    public static void RequireBuildId(string value)
    {
        if (value.Length is < 3 or > MaximumIdentifierBytes
            || !value.All(character =>
                char.IsAsciiLetterOrDigit(character) || character is ':' or '-' or '_' or '.'))
        {
            throw new DataPackFailure(
                DataPackExitCode.SourceContractMismatch,
                "game Build identity is invalid");
        }
    }

    public static void RequireIdentifier(string value, string field)
    {
        if (string.IsNullOrEmpty(value)
            || System.Text.Encoding.ASCII.GetByteCount(value) > MaximumIdentifierBytes
            || value.Any(character => !char.IsAscii(character) || char.IsControl(character)))
        {
            throw new DataPackFailure(
                DataPackExitCode.SourceContractMismatch,
                $"{field} is not a bounded ASCII identifier");
        }
    }

    public static bool IsCanonicalSha256(string? value) =>
        value is { Length: 64 }
        && value.All(character => character is >= '0' and <= '9' or >= 'a' and <= 'f');
}
