# Public deployment: dagalog + backlog dashboard behind a Google sign-in gate

Runbook for exposing this server's dagalog instance and backlog dashboard
(epic [#378](https://github.com/daghovland/rdf-datalog/issues/378)) to the
public internet, gated by Google sign-in for a specific set of allowed
accounts. Config templates live in [`deploy/`](../../deploy/).

## Why an edge proxy, not dagalog's own OIDC support

`sparql_endpoint` already has a generic OIDC resource-server mode
(`AuthConfig::Oidc`, see [`AUTH.md`](../plans/AUTH.md)) that can validate
Google-issued JWTs. It was considered and set aside for this specific use
case, for reasons worth recording:

- That mode authorizes by **role claim** (`roles_claim` → `read_role`/
  `write_role`/`admin_role`). A personal Google (`@gmail.com`) ID token
  carries no `roles` claim at all — that's an Entra ID / Google Workspace
  concept, not something a consumer Google account has. Without a role
  match, every request is denied (fails closed — never silently
  permissive), which is safe but means Google sign-in with a personal
  account currently authenticates you and then lets nothing through.
- The browser sign-in UI wired to `browser_client_id` in `frontend.html`
  only loads **MSAL.js** (Microsoft's library) today. Real "Sign in with
  Google" in the browser (Google Identity Services) isn't implemented —
  the doc comment describing `browser_client_id` as covering both is
  aspirational, not current behavior.
- Making dagalog itself a real Google OAuth resource server for this case
  would mean: extending `Claims`/`auth.rs` to check `email`+`email_verified`
  against an allowlist instead of roles; adding a *second*, separate Google
  Identity Services browser integration to both `sparql_endpoint/src/
  frontend.html` **and** `backlog_endpoint/src/backlog_frontend.html`
  (separate files, separate origins, per the
  [#381](https://github.com/daghovland/rdf-datalog/issues/381) Stage 1
  split); and wiring the dashboard's own `sparqlFetch` to forward a bearer
  token cross-origin, on top of the CORS allowlisting already fixed in
  [#435](https://github.com/daghovland/rdf-datalog/issues/435)/[#436](https://github.com/daghovland/rdf-datalog/pull/436).
  That's real, security-critical, freshly-written code immediately exposed
  to the public internet — a meaningfully bigger and riskier lift than
  gating both services once, in front, with battle-tested software.

An edge proxy (Caddy + oauth2-proxy) instead:

- adds zero new security-critical Rust code to this repo;
- covers **both** dagalog and `backlog_endpoint` uniformly — the dashboard
  has no auth story of its own today, and an edge gate fixes that for free;
- makes real browser "Sign in with Google" work immediately, using software
  whose entire job is exactly this;
- means the cross-origin CORS special-casing between dagalog and
  `backlog_endpoint` becomes moot for the public path — everything is
  same-origin behind Caddy (`dagalog --cors-allow-origin` config is still
  needed for local dev via `scripts/serve-backlog.sh`, unaffected by this).

This does **not** replace `sparql_endpoint`'s in-app OIDC/role system —
that stays available (e.g. for programmatic bearer-token clients, which a
cookie-based edge proxy doesn't serve well) and is the right foundation if/
when finer-grained, data-level access control is built — see
[#437](https://github.com/daghovland/rdf-datalog/issues/437) (fine-grained,
identity-aware access control epic), which is explicitly **not** solved by
this deployment and deliberately out of scope for it.

## Prerequisites

1. **A domain name pointed at this server.** Google's OAuth client
   configuration requires an HTTPS origin with a real hostname —
   a bare IP address (`http://203.0.113.1`) is not accepted as an
   Authorized JavaScript origin / redirect URI except for `localhost`.
   Create an `A` record for your domain (or a subdomain, e.g.
   `dagalog.yourdomain.com`) pointing at this server's public IP.
2. Docker and Docker Compose installed on this server.
3. Ports 80 and 443 open on this server's firewall (needed for Let's
   Encrypt's HTTP-01 challenge and for HTTPS itself).

## Step 1 — Create a Google OAuth client

1. Go to the [Google Cloud Console](https://console.cloud.google.com/) →
   create a project (or reuse one) → **APIs & Services → Credentials**.
2. **Create Credentials → OAuth client ID → Web application.**
3. **Authorized redirect URIs**: `https://<your-domain>/oauth2/callback`
   (must match `PUBLIC_DOMAIN` in `deploy/.env` exactly, including scheme).
4. Note the generated **Client ID** and **Client secret**.
5. If prompted to configure the OAuth consent screen, "External" user type
   is fine for a personal-account allowlist use case (you don't need Google
   Workspace verification for a small, non-public-facing consent screen
   used by a handful of named testers/users).

## Step 2 — Configure

```sh
cd deploy
cp .env.example .env
cp authenticated-emails.txt.example authenticated-emails.txt
```

Edit `.env`:
- `PUBLIC_DOMAIN` — the domain from the prerequisites step.
- `GOOGLE_OAUTH_CLIENT_ID` / `GOOGLE_OAUTH_CLIENT_SECRET` — from Step 1.
- `OAUTH2_PROXY_COOKIE_SECRET` — generate with:
  ```sh
  python3 -c 'import secrets,base64; print(base64.urlsafe_b64encode(secrets.token_bytes(32)).decode())'
  ```

Edit `authenticated-emails.txt` — one Google account email per line. This
is the actual access-control decision: oauth2-proxy lets anyone
*authenticate* to Google, but only emails listed here are let through
afterward.

Both `deploy/.env` and `deploy/authenticated-emails.txt` are gitignored —
never commit real secrets or the real allowlist.

## Step 3 — Bring it up

```sh
docker compose -f deploy/docker-compose.public.yml --env-file deploy/.env up -d --build
```

`dagalog` and `backlog-endpoint` are **not** published to the host — they're
reachable only on the internal `edge` Docker network. Only `caddy` (ports
80/443) touches the public interface. This means even a misconfigured
firewall can't accidentally expose the unauthenticated services directly.

## Step 4 — Verify the deny path before trusting this

**Do this before considering the deployment live.** From a machine that has
never signed in (a different browser profile, or `curl`, works):

```sh
curl -i https://<your-domain>/
```

Expected: a redirect (302) toward Google's sign-in page, not a 200 with
dashboard content. Also confirm a request for an email *not* in
`authenticated-emails.txt` is rejected after Google sign-in completes
(oauth2-proxy should show an "unauthorized" response, not let it through).

Only once both checks pass as expected should this be considered safe to
leave running unattended.

## Keeping the backlog/provenance data current

`dagalog`'s `--data /data/dataset.ttl` file needs to actually contain
`bl:`/`agp:` instance data (backlog issues/PRs, agent-provenance
summaries) for `/describe` (#493) and the backlog dashboard's SPARQL
queries to return anything real. That file is **not** rebuilt
automatically by Step 3 above — run
[`scripts/regenerate-production-dataset.sh`](../../scripts/regenerate-production-dataset.sh)
(issue [#565](https://github.com/daghovland/rdf-datalog/issues/565); see
[`docs/plans/WIRE_BACKLOG_SNAPSHOT_565_PLAN.md`](../plans/WIRE_BACKLOG_SNAPSHOT_565_PLAN.md)
for the design) once manually, then on a schedule:

```sh
scripts/regenerate-production-dataset.sh
```

This regenerates the backlog snapshot (`gh api`, so needs `gh` CLI
authenticated on this host — the same auth already used to build/deploy),
concatenates it with the backlog/provenance ontology and every
`provenance/summaries/*.ttl` file into `data/dataset.ttl`, and restarts the
`dagalog` service so it re-reads the new file (`--read-only` mode loads
once at startup, with no hot reload).

**Freshness split, worth knowing:** `provenance/summaries/*.ttl` only
updates on this checkout's next `git pull` (they're committed to `main` by
each PR's own workflow step), while the backlog snapshot itself refreshes
every time this script runs, independent of `git pull`. A stale checkout
still gets a fresh *snapshot* but stale *summaries*.

To run this daily without remembering to by hand, install the provided
systemd timer template (not installed automatically — see
[#687](https://github.com/daghovland/rdf-datalog/issues/687) for actually
rolling this out; it's an operational step for Dag to do deliberately, not
something a PR should flip on by itself):

**Pre-build the release binary first**, rather than letting the timer's
first run compile `backlog-regenerate` from scratch against the shared,
memory-constrained `CARGO_TARGET_DIR` (see the disk-usage note in the root
`CLAUDE.md` — this host runs on ~3.7GB and concurrent cargo builds are a
known OOM risk):

```sh
CARGO_TARGET_DIR=/home/dag/.cargo-shared-target/rdf-datalog cargo build --release -p backlog
```

```sh
sudo cp deploy/systemd/dagalog-backlog-refresh.{service,timer} /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now dagalog-backlog-refresh.timer
```

Adjust `User=`/`WorkingDirectory=` in the `.service` file first if the repo
isn't checked out at `/home/dag/rdf-datalog`. Check it actually ran with
`systemctl status dagalog-backlog-refresh.timer` and
`journalctl -u dagalog-backlog-refresh.service`.

## Notes

- **Live-tested against `dagalog.no`** (see [#477](https://github.com/daghovland/rdf-datalog/issues/477)): the deny path in Step 4 initially returned a bare `401` instead of redirecting to Google sign-in — `forward_auth` proxies oauth2-proxy's `/oauth2/auth` check response through as-is, so an explicit `@error status 401` / `handle_response` redirect to `/oauth2/start` is required inside the `forward_auth` block (see `deploy/Caddyfile`). With that fix, the full chain (deny → `/oauth2/start` → Google consent screen) works end-to-end.
- `dagalog` runs with `--read-only` in `docker-compose.public.yml` as a
  second layer of caution — even if the edge gate were somehow
  misconfigured, an authenticated-but-unintended requester still can't
  mutate the dataset. Remove `--read-only` only once you specifically want
  write access exposed, and re-verify Step 4 after doing so.
- To add more allowed users later, edit `deploy/authenticated-emails.txt`
  and restart oauth2-proxy: `docker compose -f deploy/docker-compose.public.yml --env-file deploy/.env restart oauth2-proxy`.
