# Somerville Events

An event website for Somerville, MA.

## Setup

```
# Install the actix-cli program that we use to run db migrations
cargo install sqlx-cli --no-default-features --features postgres
```

Install postgresql

https://www.postgresql.org/download/

```
cp .env.sample .env
```

Set the values in your `.env`

```
./reset_database.sh
sqlx migrate
```

Add a precommit hook for safety

```
cp pre-commit .git/hooks/
```

## Run

```
cargo run
```

## Query

```
curl -u username:password -s -F image=@examples/fuzz.jpeg http://localhost:8080/upload
```

## Deploy

Push to `main`. It will automatically build, test, and deploy the new version.

### Prerequisites

When modifying SQL queries, you must update the offline cache since the CI runner cannot connect to the database.

```bash
cargo sqlx prepare
```

You must have the GitHub secrets set up properly.

Go to your repository settings -> Secrets and variables -> Actions -> New repository secret. Add the following:

- `VPS_HOST`: The IP address or hostname of your VPS.
- `VPS_USER`: The username to SSH as (e.g., `git` or your user).
- `SSH_PRIVATE_KEY`: The private SSH key matching the public key in `~/.ssh/authorized_keys` on the VPS.
- `KNOWN_HOSTS`: The output of `ssh-keyscan <VPS_HOST>`.
- `OPENAI_API_KEY`: The test runner needs this.
