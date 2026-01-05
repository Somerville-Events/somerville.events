# Somerville Events

An event website for Somerville, MA.

## Setup

### Install rust

https://rust-lang.org/tools/install/

### Install `actix-cli`

We use it to run database migrations.

```
cargo install sqlx-cli --no-default-features --features postgres
```

### Install postgresql

https://www.postgresql.org/download/

### Setup `.env`

Copy the sample environment file and set the real values in `.env`.
Note that the role names `migrator` and `app_user` are fixed in the migrations, but you can configure their passwords in `.env`.

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

## Running

```bash
cargo run
```

The server will start at `http://localhost:8080`.

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
