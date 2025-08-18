# Somerville Events

An event website for Somerville, MA.


## Setup

```
cp .env.sample .env
```

Set the values in your `.env`

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
