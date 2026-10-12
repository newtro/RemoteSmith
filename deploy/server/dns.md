# RemoteSmith server DNS

The server's name, `RS_HOST` in [site.env](site.env.example), needs one **A** record (`RS_IPV4`) and, when the server has IPv6, one **AAAA** record (`RS_IPV6`). The examples below use the documentation values from `site.env.example`; your real ones live only in the gitignored `deploy/server/site.env`.

| Name | Type | Content | Proxy | TTL |
|---|---|---|---|---|
| `$RS_HOST` (e.g. `rs.example.com`) | A | `$RS_IPV4` (e.g. `192.0.2.10`) | DNS only | Auto |
| `$RS_HOST` | AAAA | `$RS_IPV6` (e.g. `2001:db8::10`) | DNS only | Auto |

- **DNS-only (grey cloud) if the zone is on Cloudflare.** Cloudflare's proxy only carries HTTP(S), so a proxied record would send RustDesk's raw TCP and UDP to Cloudflare and nothing would connect. The same goes for any other HTTP-only proxy or CDN.
- **The addresses are the server's own public addresses.** On a dual-stack cloud instance the IPv6 address is the one the provider assigned to the instance; it shows as a global address on its network interface (`ip -6 addr`).
- **No web server config is needed.** hbbs and hbbr speak their own protocols on their own ports; port 443 is not involved.
- **Touch only these records.** On a shared zone, a DNS change for RemoteSmith should create or change these two records and nothing else.

**Create the records before the first `install.sh`.** hbbs resolves its `--relay-servers` name once, at start. If the name doesn't resolve then, hbbs drops it and hands clients no relay until it restarts. `install.sh` refuses to run while `RS_HOST` doesn't resolve on the server.

## Procedure (Cloudflare API)

`site.env` names the zone (`CF_ZONE`) and the local file holding the Cloudflare token (`CF_CREDENTIALS_FILE`, which sets `CLOUDFLARE_API_TOKEN`). Don't echo the token. The script below loads both into the environment; run it from the repo root on the Mac.

```bash
set -a; . deploy/server/site.env; . "${CF_CREDENTIALS_FILE/#\~/$HOME}"; set +a
CF=https://api.cloudflare.com/client/v4
AUTH=(-H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" -H "Content-Type: application/json")

ZONE_ID=$(curl -fsS "${AUTH[@]}" "$CF/zones?name=$CF_ZONE" | python3 -c 'import json,sys; print(json.load(sys.stdin)["result"][0]["id"])')

# 1. Prove the name is unused. This must print an empty list.
curl -fsS "${AUTH[@]}" "$CF/zones/$ZONE_ID/dns_records?name=$RS_HOST" \
  | python3 -c 'import json,sys; print([(r["type"], r["content"]) for r in json.load(sys.stdin)["result"]])'

# 2. Create the records, DNS-only. (Explicit calls, no loops: works in bash and zsh.)
add_record() {
  curl -fsS "${AUTH[@]}" -X POST "$CF/zones/$ZONE_ID/dns_records" \
    --data "{\"type\":\"$1\",\"name\":\"$RS_HOST\",\"content\":\"$2\",\"proxied\":false,\"ttl\":1,\"comment\":\"RemoteSmith hbbs/hbbr (DNS-only: raw TCP/UDP 21115-21117)\"}" \
    | python3 -c 'import json,sys; r=json.load(sys.stdin); print(r["success"], r["result"]["type"], r["result"]["content"], "proxied=", r["result"]["proxied"])'
}
add_record A "$RS_IPV4"
[ -n "$RS_IPV6" ] && add_record AAAA "$RS_IPV6"
```

If step 1 lists anything, stop: the name is taken.

## Verify

```bash
dig +short A "$RS_HOST" @1.1.1.1      # must print $RS_IPV4 (a Cloudflare 104.x / 172.6x address means proxied: wrong)
dig +short AAAA "$RS_HOST" @1.1.1.1   # must print $RS_IPV6
deploy/server/verify-server.sh        # DNS + ports + key enforcement, using site.env
```

## Rollback

Delete the records by id:

```bash
curl -fsS "${AUTH[@]}" "$CF/zones/$ZONE_ID/dns_records?name=$RS_HOST" \
  | python3 -c 'import json,sys; [print(r["id"]) for r in json.load(sys.stdin)["result"]]'
curl -fsS "${AUTH[@]}" -X DELETE "$CF/zones/$ZONE_ID/dns_records/<id>"
```

## If the server's address changes

Check whether the public IPv4 address is static. On Lightsail, `aws lightsail get-static-ips` lists the static IPs and the instances they are attached to; an instance without one gets a new public address if it is stopped and started. If the address changes, update the A record and `RS_IPV4` in `site.env`. If the IPv6 address changes (for example, IPv6 is disabled and re-enabled on the instance), update the AAAA record and `RS_IPV6`. Clients don't need anything new: they use the hostname, and the key stays in `data/`.
