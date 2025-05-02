# Twine spool service

## Development

Setup db locally:

```sh
wrangler d1 migrations apply spool-dev -e dev
```

Run dev:

```sh
wrangler dev -e dev
```

## Production

Deployment is handled by github actions.
