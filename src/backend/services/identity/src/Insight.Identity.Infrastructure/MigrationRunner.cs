using DbUp;
using DbUp.Engine;
using DbUp.Engine.Output;
using Microsoft.Extensions.Logging;
using System.Globalization;
using System.Linq;
using System.Reflection;

namespace Insight.Identity.Infrastructure;

public static class MigrationRunner
{
    private static readonly Action<ILogger, int, Exception?> LogMigrationsApplied =
        LoggerMessage.Define<int>(
            LogLevel.Information,
            new EventId(1, nameof(LogMigrationsApplied)),
            "DbUp migrations complete: {Applied} scripts applied");

    private static readonly Action<ILogger, Exception?> LogMigrationFailed =
        LoggerMessage.Define(
            LogLevel.Error,
            new EventId(2, nameof(LogMigrationFailed)),
            "DbUp migration failed");

    public static void Run(string connectionString, ILogger logger)
    {
        // We intentionally do NOT call EnsureDatabase.For.MySqlDatabase here.
        // Per ADR-0006 the empty `identity` database + user GRANTs are
        // provisioned by the umbrella chart's bitnami MariaDB initdb
        // ConfigMap (charts/insight/templates/mariadb-initdb-scripts.yaml)
        // on the first MariaDB pod boot. The service user (`insight`)
        // only has privileges on the `identity` database and would be
        // denied access to the `mysql` system database that EnsureDatabase
        // queries to check for existence.

        DatabaseUpgradeResult result = DeployChanges.To
            .MySqlDatabase(connectionString)
            .WithScriptsEmbeddedInAssembly(
                Assembly.GetExecutingAssembly(),
                name => name.Contains(".Migrations.", StringComparison.Ordinal)
                        && name.EndsWith(".sql", StringComparison.OrdinalIgnoreCase))
            .LogTo(new MicrosoftLoggingAdapter(logger))
            .Build()
            .PerformUpgrade();

        if (!result.Successful)
        {
            LogMigrationFailed(logger, result.Error);
            throw new InvalidOperationException("Identity schema migration failed", result.Error);
        }

        LogMigrationsApplied(logger, result.Scripts.Count(), null);
    }

    private sealed class MicrosoftLoggingAdapter(ILogger logger) : IUpgradeLog
    {
        public void LogInformation(string format, params object[] args)
            => Emit(LogLevel.Information, null, format, args);

        public void LogWarning(string format, params object[] args)
            => Emit(LogLevel.Warning, null, format, args);

        public void LogError(string format, params object[] args)
            => Emit(LogLevel.Error, null, format, args);

        public void LogError(Exception ex, string format, params object[] args)
            => Emit(LogLevel.Error, ex, format, args);

        public void LogTrace(string format, params object[] args)
            => Emit(LogLevel.Trace, null, format, args);

        public void LogDebug(string format, params object[] args)
            => Emit(LogLevel.Debug, null, format, args);

        private void Emit(LogLevel level, Exception? ex, string format, object[] args)
        {
            if (!logger.IsEnabled(level)) return;
            string message = args is null || args.Length == 0
                ? format
                : string.Format(CultureInfo.InvariantCulture, format, args);
#pragma warning disable CA1848
            logger.Log(level, default, ex, "{Message}", message);
#pragma warning restore CA1848
        }
    }
}
