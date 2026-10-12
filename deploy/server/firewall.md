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

The VM has two firewall layers, and both must allow the ports: the Lightsail instance firewall, then ufw on the host. Facts verified 2026-10-12:

- **Lightsail:** instance `Ubuntu-1`, region `us-east-1`, dual-stack. Open today: TCP 22, 80 and 443, each with `0.0.0.0/0` and `::/0`.
- **Host:** ufw is active with `Default: deny (incoming)`. It allows only 22, 80 and 443, over v4 and v6, through the iptables-nft backend. The two other nftables tables on the VM don't affect inbound 2111x and stay untouched: `inet f2b-table` (fail2ban sshd bans) and `inet tradesmith_ai_egress` (output, for one uid).

## 1. Lightsail instance firewall

Use `OpenInstancePublicPorts`. It **adds** one rule. Do not use `PutInstancePublicPorts`, which replaces the whole rule set and would drop 22, 80 and 443.

You need credentials that can change Lightsail networking. The `remail` profile has no Lightsail access, and the `cartersworld` IAM user is scoped to Lightsail reads plus SSM, so it probably can't open ports either. Use the `aws-toolkit` profile after `aws login --profile aws-toolkit`. On 2026-10-12 that login had expired: the MCP returned "Failed to create connection for profile 'aws-toolkit'".

**AWS MCP (`run_script`, profile `aws-toolkit`):**

```python
rules = [
    {"fromPort": 21115, "toPort": 21117, "protocol": "tcp", "cidrs": ["0.0.0.0/0"], "ipv6Cidrs": ["::/0"]},
    {"fromPort": 21116, "toPort": 21116, "protocol": "udp", "cidrs": ["0.0.0.0/0"], "ipv6Cidrs": ["::/0"]},
]
out = []
for r in rules:
    resp = await call_boto3(service_name="lightsail", operation_name="OpenInstancePublicPorts",
                            region_name="us-east-1",
                            params={"instanceName": "Ubuntu-1", "portInfo": r})
    out.append(resp["operation"]["status"])
states = await call_boto3(service_name="lightsail", operation_name="GetInstancePortStates",
                          region_name="us-east-1", params={"instanceName": "Ubuntu-1"})
result = {"ops": out, "ports": [(p["fromPort"], p["toPort"], p["protocol"], p["state"],
                                 p.get("cidrs"), p.get("ipv6Cidrs")) for p in states["portStates"]]}
result
```

**Equivalent AWS CLI:**

```bash
aws lightsail open-instance-public-ports --profile aws-toolkit --region us-east-1 \
  --instance-name Ubuntu-1 \
  --port-info '{"fromPort":21115,"toPort":21117,"protocol":"tcp","cidrs":["0.0.0.0/0"],"ipv6Cidrs":["::/0"]}'
aws lightsail open-instance-public-ports --profile aws-toolkit --region us-east-1 \
  --instance-name Ubuntu-1 \
  --port-info '{"fromPort":21116,"toPort":21116,"protocol":"udp","cidrs":["0.0.0.0/0"],"ipv6Cidrs":["::/0"]}'
aws lightsail get-instance-port-states --profile aws-toolkit --region us-east-1 --instance-name Ubuntu-1
```

**Expected afterwards:** 22, 80 and 443 unchanged, plus `21115-21117 tcp open` and `21116-21116 udp open`, each with `0.0.0.0/0` and `::/0`. Nothing mentions 21114, 21118 or 21119.

**Rollback:** run `CloseInstancePublicPorts` (or `aws lightsail close-instance-public-ports`) with the same two `portInfo` values.

## 2. Host firewall (ufw)

`deploy/server/firewall.sh` does this, then checks the result. On the VM:

```bash
sudo /opt/remotesmith-server/kit/firewall.sh            # after install.sh; or the repo copy
sudo /opt/remotesmith-server/kit/firewall.sh --check    # prints the rules; non-zero if wrong
```

The script runs exactly these commands. ufw adds the IPv6 twin of each rule itself:

```bash
sudo ufw allow 21115:21117/tcp comment 'remotesmith hbbs/hbbr'
sudo ufw allow 21116/udp       comment 'remotesmith hbbs/hbbr'
```

- **Not affected:** `ufw allow` reloads only ufw's user chains, so existing connections and other services keep running. Nothing else on the box listens on 21114–21119 (checked with `ss -ltnup` on 2026-10-12).
- **Rollback:** `sudo ./firewall.sh --remove`, which runs `ufw delete allow 21115:21117/tcp` and `ufw delete allow 21116/udp`.

## 3. Verify from outside

On the Mac, once DNS exists (dns.md):

```bash
deploy/server/verify-server.sh
```

It checks that 21115, 21116 and 21117 are open, that UDP 21116 answers, and that 21114, 21118 and 21119 are **not** reachable. It also runs the key-enforcement probes.
