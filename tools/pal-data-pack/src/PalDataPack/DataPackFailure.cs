namespace PalDataPack;

public enum DataPackExitCode
{
    Success = 0,
    Usage = 2,
    SourceContractMismatch = 20,
    AssetContractMismatch = 21,
    MountOrSerialization = 22,
    ReferenceIntegrity = 23,
    PackageIntegrity = 24,
    Publication = 25,
}

public sealed class DataPackFailure : Exception
{
    public DataPackFailure(DataPackExitCode exitCode, string message)
        : base(message)
    {
        ExitCode = exitCode;
    }

    public DataPackExitCode ExitCode { get; }
}
