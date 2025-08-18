# Somerville Events

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
