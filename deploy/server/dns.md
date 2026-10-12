# RemoteSmith server DNS

`rs.johnnycode.ai` needs one **A** record and one **AAAA** record pointing at the Lightsail VM (`Ubuntu-1`). Both must be **DNS-only (grey cloud)**. Cloudflare's proxy only carries HTTP(S), so a proxied record would send RustDesk's raw TCP and UDP to Cloudflare and nothing would connect.

| Name | Type | Content | Proxy | TTL |
|---|---|---|---|---|
| `rs.johnnycode.ai` | A | `98.93.243.79` | DNS only | Auto |
| `rs.johnnycode.ai` | AAAA | `2600:1f18:72b8:5500:81e8:9091:9439:f3ef` | DNS only | Auto |

Facts verified 2026-10-12:

- **Zone:** `johnnycode.ai` is on Cloudflare (nameservers `rita` and `zahir.ns.cloudflare.com`).
- **No existing record:** `rs.johnnycode.ai` has no A or AAAA record today.
- **Addresses:** the IPv4 address is the VM's public address. The IPv6 address is the one Lightsail assigned to `Ubuntu-1` (dual-stack), and it shows as global on `ens5`.
- **Not touched:** no Caddy site block is needed, because hbbs and hbbr speak their own protocols on their own ports. Port 443 is not involved.

Per the `aws-vm` skill, this is a DNS change, so it is made only with Scott's approval, and it touches **only** these two records.

**Create the records before the first `install.sh`.** hbbs resolves its `--relay-servers` name once, at start. If the name doesn't resolve then, hbbs drops it and hands clients no relay until it restarts. `install.sh` refuses to run while `rs.johnnycode.ai` doesn't resolve on the VM.

## Procedure (Cloudflare API)

The Cloudflare token lives in `~/.config/cloudflare/credentials.env` on the Mac (`CLOUDFLARE_API_TOKEN`, `CLOUDFLARE_ACCOUNT_ID`). Don't echo it. The script below sources it into the environment.

```bash
set -a; . ~/.config/cloudflare/credentials.env; set +a
CF=https://api.cloudflare.com/client/v4
AUTH=(-H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" -H "Content-Type: application/json")

ZONE_ID=$(curl -fsS "${AUTH[@]}" "$CF/zones?name=johnnycode.ai" | python3 -c 'import json,sys; print(json.load(sys.stdin)["result"][0]["id"])')

# 1. Prove the name is unused. This must print an empty list.
curl -fsS "${AUTH[@]}" "$CF/zones/$ZONE_ID/dns_records?name=rs.johnnycode.ai" \
  | python3 -c 'import json,sys; print([(r["type"], r["content"]) for r in json.load(sys.stdin)["result"]])'

# 2. Create the two records, DNS-only. (Explicit calls, no loops: works in bash and zsh.)
add_record() {
  curl -fsS "${AUTH[@]}" -X POST "$CF/zones/$ZONE_ID/dns_records" \
    --data "{\"type\":\"$1\",\"name\":\"rs.johnnycode.ai\",\"content\":\"$2\",\"proxied\":false,\"ttl\":1,\"comment\":\"RemoteSmith hbbs/hbbr (DNS-only: raw TCP/UDP 21115-21117)\"}" \
    | python3 -c 'import json,sys; r=json.load(sys.stdin); print(r["success"], r["result"]["type"], r["result"]["content"], "proxied=", r["result"]["proxied"])'
}
add_record A 98.93.243.79
add_record AAAA 2600:1f18:72b8:5500:81e8:9091:9439:f3ef
```

If step 1 lists anything, stop and ask: the name is taken.

## Verify

```bash
dig +short A rs.johnnycode.ai @1.1.1.1      # 98.93.243.79   (a 104.x / 172.6x address means proxied: wrong)
dig +short AAAA rs.johnnycode.ai @1.1.1.1   # 2600:1f18:72b8:5500:81e8:9091:9439:f3ef
deploy/server/verify-server.sh              # DNS + ports + key enforcement
```

## Rollback

Delete the two records by id:

```bash
curl -fsS "${AUTH[@]}" "$CF/zones/$ZONE_ID/dns_records?name=rs.johnnycode.ai" \
  | python3 -c 'import json,sys; [print(r["id"]) for r in json.load(sys.stdin)["result"]]'
curl -fsS "${AUTH[@]}" -X DELETE "$CF/zones/$ZONE_ID/dns_records/<id>"
```

## If the VM's address changes

`98.93.243.79` is **not** a Lightsail static IP. `get-static-ips` on 2026-10-12 lists only `fi-playtest-static`, which belongs to `Ubuntu-2`. The address would change if the instance were ever stopped and started. That risk already applies to every site on the box and to `ssh vm`, so it isn't specific to RemoteSmith. If it happens, update the A record. If Lightsail's IPv6 changes (for example, IPv6 is disabled and re-enabled on the instance), update the AAAA record. Clients don't need anything new: they use the hostname, and the key stays in `data/`.
