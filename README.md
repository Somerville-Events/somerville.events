# Somerville Events

An event website for Somerville, MA.

## Setup

### Install rust

https://rust-lang.org/tools/install/

### Install `sqlx-cli`

We use it to run database migrations.

```
cargo install sqlx-cli --no-default-features --features postgres
```

### Install postgresql

https://www.postgresql.org/download/

### Setup `.env`

Copy the sample environment file and set the real values in `.env`.

**Security Note:** For local development, keep `DB_MIGRATOR_PASS` in `.env`. For production, remove `DB_MIGRATOR_PASS` from the `.env` file on the server; it will be injected securely by the deployment pipeline.

```bash
cp .env.sample .env
```

### Initialize the database

```bash
./reset_database.sh
sqlx migrate run
```

_Note: `reset_database.sh` drops and recreates the database using the credentials in `.env`._

### Add the precommit hook

This runs some safety checks before pushing to main.

```bash
cp pre-commit .git/hooks/pre-commit
```

### Security Checks

We use `cargo-audit` to check for vulnerabilities in dependencies.

1. **Install:** `cargo install cargo-audit --features=fix`
2. **Run:** `cargo audit`
3. **Fix:**
   - **Autofix:** Run `cargo audit fix` to automatically upgrade vulnerable dependencies to safe versions.
   - **Manual Update:** Run `cargo update` to pull in patched versions if autofix doesn't work.
   - **Ignore:** If a vulnerability is a false positive (e.g. unused feature), add the ID to `.cargo/audit.toml` under `ignore` with a comment explaining why.

## Running

```bash
cargo run
```

The server will start at `http://localhost:8080`.

## Running the Ingestor

The ingestor fetches events from external sources and saves them to the database.

```bash
cargo run --bin ingest_events
```

It can also be run in dry-run mode (no database writes, no API costs):

```bash
cargo run --bin ingest_events -- --dry-run
```

## UI Development (Storybook)

We use mocked UI templates to develop the UI in isolation without running the full backend or database. This allows for rapid iteration and testing of edge cases.

### Running Storybook

```bash
cargo run --example storybook
```

### Auto-reload with `cargo-watch`

For the best developer experience, use `cargo-watch` to automatically recompile and restart the server when you change templates or code.

1. **Install:** `cargo install cargo-watch`
2. **Run:**
   ```bash
   cargo watch -x 'run --example storybook'
   ```

## Deployment

Push to `main`. GitHub Actions will build, test, and deploy to the VPS.

You must have the GitHub secrets set up properly.

Go to your repository settings -> Secrets and variables -> Actions -> New repository secret. Add the following:

- `VPS_HOST`: The IP address or hostname of your VPS.
- `VPS_USER`: The username to SSH as (e.g., `git` or your user).
- `SSH_PRIVATE_KEY`: The private SSH key matching the public key in `~/.ssh/authorized_keys` on the VPS.
- `KNOWN_HOSTS`: The output of `ssh-keyscan <VPS_HOST>`.
- `OPENAI_API_KEY`: The test runner needs this.
- `DB_MIGRATOR_PASS`: The password for the `migrator` database role. This is injected during CI and deployment and should NOT be in the `.env` file on the server.

## Cron Job Setup

To keep events up to date, you should set up a cron job to run the ingestor periodically. See [CRON_SETUP.md](CRON_SETUP.md) for instructions.
