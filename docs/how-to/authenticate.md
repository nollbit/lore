# Authenticate the CLI

For a server with an `auth_url` configured, run:

```sh
lore login lores://lore.example.com:41337
```

Complete the approval in your browser. Use `--no-browser` to print the login URL
without opening it automatically.

The CLI saves credentials in its encrypted credential store. When the auth
provider issues a refresh token, repository commands renew expired identity
tokens automatically. Concurrent processes sharing the store serialize refresh
requests and persist the identity and refresh token together.

The UCS authentication protocol carries an optional `refresh_token` in
`UserToken` and accepts it in `RefreshAuthSessionRequest.refresh_token`. A
successful refresh must return a valid identity token for the same user. A
provider that rotates refresh tokens returns the replacement in `UserToken`.
If no replacement is returned, the CLI retains the existing refresh token.
Token and session lifetimes are controlled by the auth provider.

Explicit `--identity-token` and `--access-token` credentials do not use stored
refresh tokens. Providers that do not issue refresh tokens require another
`lore login` after the identity token expires.

`lore logout` removes local credentials. Use the auth provider's session controls
to revoke a session on the server. An expired or revoked refresh session requires
another browser login.
