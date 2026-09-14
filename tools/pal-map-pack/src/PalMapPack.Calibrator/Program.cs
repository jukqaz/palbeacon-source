namespace PalMapPack.Calibrator;

internal interface ICalibratorLaunchHost
{
    int RunStandard(string? expectedBuild);

    int RunLocalAlignmentCapture(LocalAlignmentCaptureCommand command);
}

internal static class Program
{
    [STAThread]
    private static int Main(string[] args)
    {
        ApplicationConfiguration.Initialize();
        try
        {
            return Dispatch(args, new WindowsCalibratorLaunchHost());
        }
        catch (Exception error) when (ShouldHandle(error, args))
        {
            MessageBox.Show(
                error.Message,
                "Exact Build detection failed",
                MessageBoxButtons.OK,
                MessageBoxIcon.Error);
            return ErrorExitCode(args);
        }
    }

    internal static int Dispatch(
        IReadOnlyList<string> args,
        ICalibratorLaunchHost host)
    {
        ArgumentNullException.ThrowIfNull(args);
        ArgumentNullException.ThrowIfNull(host);
        if (HasCaptureIntent(args))
        {
            return host.RunLocalAlignmentCapture(
                LocalAlignmentCaptureCommand.Parse(args));
        }

        return host.RunStandard(ParseExpectedBuild(args));
    }

    internal static int ErrorExitCode(IReadOnlyList<string> args)
    {
        ArgumentNullException.ThrowIfNull(args);
        return HasCaptureIntent(args) ? 1 : 0;
    }

    internal static bool ShouldHandle(
        Exception error,
        IReadOnlyList<string> args)
    {
        ArgumentNullException.ThrowIfNull(error);
        ArgumentNullException.ThrowIfNull(args);
        if (error is ArgumentException
            or InvalidOperationException
            or MapPackFailure)
        {
            return true;
        }

        return HasCaptureIntent(args)
            && error is IOException
                or UnauthorizedAccessException
                or OutOfMemoryException;
    }

    private static string? ParseExpectedBuild(IReadOnlyList<string> args)
    {
        if (args.Count == 0)
        {
            return null;
        }
        if (args.Count != 2
            || args[0] != "--build"
            || args[1].Length == 0
            || args[1].Any(character => !char.IsAsciiDigit(character)))
        {
            throw new ArgumentException(
                "usage: PalMapPack.Calibrator.exe [--build <exact numeric Build ID>]");
        }
        return args[1];
    }

    private static bool HasCaptureIntent(IReadOnlyList<string> args) =>
        args.Any(argument => string.Equals(
            argument,
            "--local-alignment-capture",
            StringComparison.Ordinal));
}

internal sealed class WindowsCalibratorLaunchHost : ICalibratorLaunchHost
{
    public int RunStandard(string? expectedBuild)
    {
        var detectedBuild = SteamInstallLocator.LocateDefault("1623730").BuildId;
        if (expectedBuild is not null && expectedBuild != detectedBuild)
        {
            throw new InvalidOperationException(
                "requested Build does not match the installed Steam Build");
        }

        Application.Run(new MainForm(detectedBuild));
        return 0;
    }

    public int RunLocalAlignmentCapture(LocalAlignmentCaptureCommand command)
    {
        using var form = new LocalAlignmentCaptureForm(command);
        _ = form.ShowDialog();
        return form.ExitCode;
    }
}
