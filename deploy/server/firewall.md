# RemoteSmith server firewall

The server needs exactly **TCP 21115–21117** and **UDP 21116**, on IPv4 and IPv6 (PLAN §4, DEF-14):

| Port | Proto | Daemon | Purpose |
|---|---|---|---|
| 21115 | TCP | hbbs | NAT type test |
| 21116 | TCP + UDP | hbbs | ID registration, heartbeat, hole punching, connection requests |
| 21117 | TCP | hbbr | relay |

These stay **closed**:

| Port | Why |
|---|---|
| 21114 | the Pro web console; OSS has none |
| 21118 | the hbbs WebSocket listener. hbbs always binds it, on all interfaces. Closing it at the firewall is the only control. It also trusts `X-Real-IP` headers. |
| 21119 | the hbbr WebSocket listener. It is always bound too. |

The kit assumes two firewall layers, and both must allow the ports: the cloud provider's instance firewall (Lightsail here: `LIGHTSAIL_INSTANCE` and `LIGHTSAIL_REGION` in [site.env](site.env.example)), then ufw on the host. Before changing either, record what is open today:

- **Lightsail:** `aws lightsail get-instance-port-states --instance-name "$LIGHTSAIL_INSTANCE"`. A typical web host has TCP 22, 80 and 443 open, each with `0.0.0.0/0` and `::/0`.
- **Host:** `sudo ufw status verbose` must show `Status: active` and `Default: deny (incoming)`; `firewall.sh` refuses to guess otherwise. Other nftables tables (fail2ban's, per-app egress rules) don't affect inbound 2111x and stay untouched: list them with `sudo nft list tables`.

## 1. Lightsail instance firewall

Use `OpenInstancePublicPorts`. It **adds** one rule. Do not use `PutInstancePublicPorts`, which replaces the whole rule set and would drop 22, 80 and 443.

You need credentials that can change Lightsail networking (`lightsail:OpenInstancePublicPorts`); read-only or SSM-scoped users can't. `AWS_PROFILE` in `site.env` names the profile to use; sign it in first (`aws login --profile "$AWS_PROFILE"`).

**AWS MCP (`run_script`, with the profile from `AWS_PROFILE`).** The script can't read `site.env`, so put your `LIGHTSAIL_INSTANCE` and `LIGHTSAIL_REGION` values in the first line:

```python
instance, region = "my-instance", "us-east-1"   # LIGHTSAIL_INSTANCE, LIGHTSAIL_REGION from site.env
rules = [
    {"fromPort": 21115, "toPort": 21117, "protocol": "tcp", "cidrs": ["0.0.0.0/0"], "ipv6Cidrs": ["::/0"]},
    {"fromPort": 21116, "toPort": 21116, "protocol": "udp", "cidrs": ["0.0.0.0/0"], "ipv6Cidrs": ["::/0"]},
]
out = []
for r in rules:
    resp = await call_boto3(service_name="lightsail", operation_name="OpenInstancePublicPorts",
                            region_name=region,
                            params={"instanceName": instance, "portInfo": r})
    out.append(resp["operation"]["status"])
states = await call_boto3(service_name="lightsail", operation_name="GetInstancePortStates",
                          region_name=region, params={"instanceName": instance})
result = {"ops": out, "ports": [(p["fromPort"], p["toPort"], p["protocol"], p["state"],
                                 p.get("cidrs"), p.get("ipv6Cidrs")) for p in states["portStates"]]}
result
```

**Equivalent AWS CLI** (from the repo root on the Mac):

```bash
set -a; . deploy/server/site.env; set +a
LS=(--profile "$AWS_PROFILE" --region "$LIGHTSAIL_REGION" --instance-name "$LIGHTSAIL_INSTANCE")
aws lightsail open-instance-public-ports "${LS[@]}" \
  --port-info '{"fromPort":21115,"toPort":21117,"protocol":"tcp","cidrs":["0.0.0.0/0"],"ipv6Cidrs":["::/0"]}'
aws lightsail open-instance-public-ports "${LS[@]}" \
  --port-info '{"fromPort":21116,"toPort":21116,"protocol":"udp","cidrs":["0.0.0.0/0"],"ipv6Cidrs":["::/0"]}'
aws lightsail get-instance-port-states "${LS[@]}"
```

**Expected afterwards:** the rules that were open before unchanged, plus `21115-21117 tcp open` and `21116-21116 udp open`, each with `0.0.0.0/0` and `::/0`. Nothing mentions 21114, 21118 or 21119.

**Rollback:** run `CloseInstancePublicPorts` (or `aws lightsail close-instance-public-ports`) with the same two `portInfo` values.

## 2. Host firewall (ufw)

`deploy/server/firewall.sh` does this, then checks the result. On the server:

```bash
sudo /opt/remotesmith-server/kit/firewall.sh            # after install.sh; or the repo copy
sudo /opt/remotesmith-server/kit/firewall.sh --check    # prints the rules; non-zero if wrong
```

The script runs exactly these commands. ufw adds the IPv6 twin of each rule itself:

```bash
sudo ufw allow 21115:21117/tcp comment 'remotesmith hbbs/hbbr'
sudo ufw allow 21116/udp       comment 'remotesmith hbbs/hbbr'
```

- **Not affected:** `ufw allow` reloads only ufw's user chains, so existing connections and other services keep running. Check first that nothing else on the box listens on 21114–21119 (`ss -ltnup`); `install.sh` refuses a first install if something does.
- **Rollback:** `sudo ./firewall.sh --remove`, which runs `ufw delete allow 21115:21117/tcp` and `ufw delete allow 21116/udp`.

## 3. Verify from outside

On the Mac, once DNS exists (dns.md), with `site.env` filled in:

```bash
deploy/server/verify-server.sh
```

It checks that 21115, 21116 and 21117 are open, that UDP 21116 answers, and that 21114, 21118 and 21119 are **not** reachable. It also runs the key-enforcement probes.
