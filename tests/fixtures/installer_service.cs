// A real SCM service, plus a foreground process, for installer lifecycle tests.
// Compatible with the C# compiler included in Windows' .NET Framework 4.
using System.ServiceProcess;
using System.Threading;

internal sealed class InstallerService : ServiceBase
{
    private readonly bool hangOnStop;

    private InstallerService(string name, bool hang)
    {
        ServiceName = name;
        CanStop = true;
        AutoLog = false;
        hangOnStop = hang;
    }

    protected override void OnStart(string[] args)
    {
    }

    protected override void OnStop()
    {
        // Simulate a service that never completes a stop request.
        if (hangOnStop)
            Thread.Sleep(Timeout.Infinite);
    }

    private static void Main(string[] args)
    {
        if (args.Length >= 2 && args[0] == "--service")
        {
            using (var service = new InstallerService(
                args[1], args.Length >= 3 && args[2] == "--hang"))
                ServiceBase.Run(service);
        }
        else
        {
            Thread.Sleep(Timeout.Infinite);
        }
    }
}
