# Somerville Events

An event website for Somerville, MA.

## Setup

```
cargo install sqlx-cli
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

## Develop

Push to main. It should automatically build and run your new version on the VPS.

```
git checkout main
# make changes
git push
```
