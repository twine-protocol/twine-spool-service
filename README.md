# Twine spool service

## Development

Setup db locally:

```sh
wrangler d1 execute spool-dev -e dev --local --file=./schema.sql
```

Run dev:

```sh
wrangler dev -e dev
```

## Production

Setup remotely

```sh
wrangler d1 execute spool-prod --remote --file=./schema.sql
```

Put secret key

```sh
echo some_key_text | wrangler secret put SECRET_KEY_STR
```

Deploy

```sh
wrangler deploy
```

## TODO

- [] Setup github deployment
- [] Setup deploy to cloudflare button
- [] Randomness example
- [] Fix d1 store to use resolver interface
- [] check put/push api key checking

