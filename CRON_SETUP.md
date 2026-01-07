# Cron Job Setup for Event Ingestion

The event ingestor (`ingest_events`) is designed to run periodically to fetch new events from external sources.

## Prerequisites

- The `somerville-events` application must be deployed (see `README.md` and `deploy.sh`).
- The `ingest_events` binary should be available at `~/bin/ingest_events` (created by `deploy.sh`).
- Environment variables must be set (usually in `~/.env`).

## Setting up the Cron Job

1.  SSH into your VPS.
2.  Open the crontab editor:
    ```bash
    crontab -e
    ```
3.  Add the following line to run the ingestor every 6 hours (adjust frequency as needed):

    ```cron
    0 */6 * * * . $HOME/.env && $HOME/bin/ingest_events >> $HOME/ingest.log 2>&1
    ```

    **Explanation:**
    - `. $HOME/.env`: Sources the environment variables (DB credentials, API keys) required by the application.
    - `$HOME/bin/ingest_events`: Runs the ingestor binary.
    - `>> $HOME/ingest.log 2>&1`: Appends standard output and error logs to `ingest.log` for debugging.

## Verifying

You can check if the job ran by inspecting the log file:

```bash
tail -f $HOME/ingest.log
```

Or checking the cron logs (depending on your OS, e.g., `/var/log/syslog` or `journalctl`).
