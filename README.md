# steam_crawler

A crawler to scrape game information from steam.

## Usage

With number of titles to return:

```sh
cargo run -- --count 50 400 50
```

With maximum runtime (seconds):

```sh
cargo run -- --time 120 400 50
```

The last two numbers are the "seed" game IDs, i.e. the games to start from.