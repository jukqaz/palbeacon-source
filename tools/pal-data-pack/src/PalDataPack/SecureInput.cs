namespace PalDataPack;

public static class SecureInput
{
    public static byte[] ReadBoundedRegularFile(string path, int maximumBytes)
    {
        var fullPath = Path.GetFullPath(path);
        EnsureRegularFile(fullPath);
        var info = new FileInfo(fullPath);
        if (info.Length is <= 0 || info.Length > maximumBytes)
        {
            throw new DataPackFailure(
                DataPackExitCode.SourceContractMismatch,
                "input file size is outside the allowed bound");
        }
        return File.ReadAllBytes(fullPath);
    }

    public static void EnsureRegularFile(string path)
    {
        var info = new FileInfo(path);
        if (!info.Exists
            || (info.Attributes & (FileAttributes.Directory | FileAttributes.ReparsePoint))
                != 0)
        {
            throw new DataPackFailure(
                DataPackExitCode.SourceContractMismatch,
                "input must be an existing non-reparse regular file");
        }
    }

    public static void EnsurePlainDirectory(string path)
    {
        var fullPath = Path.GetFullPath(path);
        var current = new DirectoryInfo(fullPath);
        while (current is not null)
        {
            if (current.Exists
                && (current.Attributes & FileAttributes.ReparsePoint) != 0)
            {
                throw new DataPackFailure(
                    DataPackExitCode.SourceContractMismatch,
                    "input directory cannot traverse a reparse point");
            }
            current = current.Parent;
        }
        if (!Directory.Exists(fullPath))
        {
            throw new DataPackFailure(
                DataPackExitCode.SourceContractMismatch,
                "input directory does not exist");
        }
    }
}
