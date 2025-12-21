# Somerville Events

An event website for Somerville, MA.

## Setup

```
# Install the actix-cli program that we use to run db migrations
# including rustls for tls and support for postgres
cargo install sqlx-cli --no-default-features --features rustls,postgres
```

Install postgresql

https://www.postgresql.org/download/

```
cp .env.sample .env
```

Set the values in your `.env`

```
./reset_database.sh
```

## Run

```
cargo run
```

## Query

```
curl -u username:password -s -F image=@examples/fuzz.jpeg http://localhost:8080/upload
```

## Deployment

The project uses GitHub Actions to build and deploy to the VPS.

### 1. Prerequisites (Local)

When modifying SQL queries, you must update the offline cache since the CI runner cannot connect to the database.

```bash
cargo sqlx prepare -- --lib
git add .sqlx
```

### 2. GitHub Secrets Configuration

Go to your repository settings -> Secrets and variables -> Actions -> New repository secret. Add the following:

- `VPS_HOST`: The IP address or hostname of your VPS.
- `VPS_USER`: The username to SSH as (e.g., `git` or your user).
- `SSH_PRIVATE_KEY`: The private SSH key matching the public key in `~/.ssh/authorized_keys` on the VPS.
- `KNOWN_HOSTS`: The output of `ssh-keyscan <VPS_HOST>`.

### 3. Deploying

Push to `main`. It will automatically build, test, and deploy the new version.

```bash
git checkout main
# make changes
cargo sqlx prepare -- --lib # if you changed queries
git push
```
