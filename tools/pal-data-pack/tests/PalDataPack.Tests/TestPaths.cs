namespace PalDataPack.Tests;

internal static class TestPaths
{
    public static string RepositoryRoot()
    {
        var current = new DirectoryInfo(AppContext.BaseDirectory);
        while (current is not null)
        {
            if (File.Exists(Path.Combine(current.FullName, "Cargo.toml"))
                && Directory.Exists(Path.Combine(current.FullName, "tests", "fixtures")))
            {
                return current.FullName;
            }
            current = current.Parent;
        }
        throw new DirectoryNotFoundException("repository root was not found");
    }

    public static string SyntheticCatalog() => Path.Combine(
        RepositoryRoot(),
        "tests",
        "fixtures",
        "local-data",
        "catalog-synthetic-v1");

    public static string TempDirectory()
    {
        var path = Path.Combine(
            Path.GetTempPath(),
            "pal-data-pack-tests",
            Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(path);
        return path;
    }

    public static string CopySyntheticCatalog(string destination)
    {
        Directory.CreateDirectory(destination);
        foreach (var source in Directory.EnumerateFiles(SyntheticCatalog()))
        {
            File.Copy(source, Path.Combine(destination, Path.GetFileName(source)));
        }
        return destination;
    }
}
