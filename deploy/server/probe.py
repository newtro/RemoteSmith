#!/usr/bin/env python3
"""Wire-level probes for a RustDesk OSS server (hbbs/hbbr 1.1.16).

Standard library only. Messages are hand-encoded protobuf (hbb_common
protos/rendezvous.proto) so no client build is needed.

  probe.py udp-register HOST [PORT]   UDP RegisterPeer for a random unknown ID.
                                      hbbs answers RegisterPeerResponse. The ID is
                                      unknown, so hbbs stores nothing.
  probe.py punch HOST KEY [PORT]      TCP PunchHoleRequest for a random unknown ID,
                                      carrying KEY as licence_key ("" = no key).
                                      Prints the PunchHoleResponse failure name.
                                      LICENSE_MISMATCH means hbbs refused the key.
  probe.py tcp HOST PORT              TCP connect. Prints "open", "refused", "filtered"
                                      or "unreachable".
  probe.py relay HOST KEY [PORT]      TCP RequestRelay with a random uuid, carrying KEY.
                                      Prints "refused" if hbbr closes the connection
                                      at once (key rejected), or "accepted" if it holds
                                      the connection open waiting for a peer (key OK).

Exit status: 0 when the probe got an answer it could classify, 2 on a network or
protocol error. The caller decides which classification is a pass.
"""

import os
import socket
import sys
import time

FAILURES = {0: "ID_NOT_EXIST", 2: "OFFLINE", 3: "LICENSE_MISMATCH", 4: "LICENSE_OVERUSE"}


def varint(n):
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        if n:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


def field_bytes(num, data):
    return varint((num << 3) | 2) + varint(len(data)) + data


def field_str(num, s):
    return b"" if s == "" else field_bytes(num, s.encode())


def parse(buf):
    """Parse one protobuf message into {field: [values]} (varints and length-delimited only)."""
    out = {}
    i = 0
    while i < len(buf):
        key, i = read_varint(buf, i)
        num, wt = key >> 3, key & 7
        if wt == 0:
            val, i = read_varint(buf, i)
        elif wt == 2:
            ln, i = read_varint(buf, i)
            val = buf[i:i + ln]
            i += ln
        elif wt == 5:
            val = buf[i:i + 4]
            i += 4
        elif wt == 1:
            val = buf[i:i + 8]
            i += 8
        else:
            raise ValueError("unsupported wire type %d" % wt)
        out.setdefault(num, []).append(val)
    return out


def read_varint(buf, i):
    shift = 0
    n = 0
    while True:
        if i >= len(buf):
            raise ValueError("truncated varint")
        b = buf[i]
        i += 1
        n |= (b & 0x7F) << shift
        if not b & 0x80:
            return n, i
        shift += 7


def frame(data):
    """hbb_common BytesCodec framing: length << 2 with the head size in the low 2 bits."""
    n = len(data)
    if n <= 0x3F:
        head = bytes([n << 2])
    elif n <= 0x3FFF:
        head = ((n << 2) | 1).to_bytes(2, "little")
    elif n <= 0x3FFFFF:
        head = ((n << 2) | 2).to_bytes(3, "little")
    else:
        head = ((n << 2) | 3).to_bytes(4, "little")
    return head + data


def read_frame(sock):
    first = recv_exact(sock, 1)
    head_len = (first[0] & 3) + 1
    head = first + recv_exact(sock, head_len - 1)
    n = int.from_bytes(head, "little") >> 2
    return recv_exact(sock, n)


def recv_exact(sock, n):
    buf = b""
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise ConnectionError("connection closed by server")
        buf += chunk
    return buf


def rand_id():
    return "rsprobe" + os.urandom(4).hex()


def udp_register(host, port):
    msg = field_bytes(6, field_str(1, rand_id()))  # RendezvousMessage.register_peer
    family, _, _, _, addr = socket.getaddrinfo(host, port, type=socket.SOCK_DGRAM)[0]
    s = socket.socket(family, socket.SOCK_DGRAM)
    s.settimeout(5)
    try:
        s.sendto(msg, addr)
        data, _ = s.recvfrom(4096)
    finally:
        s.close()
    m = parse(data)
    if 7 in m:  # register_peer_response
        print("register_peer_response")
        return 0
    print("unexpected-response fields=%s" % sorted(m))
    return 2


def punch(host, key, port):
    ph = field_str(1, rand_id()) + field_str(3, key)
    msg = field_bytes(8, ph)  # RendezvousMessage.punch_hole_request
    with socket.create_connection((host, port), timeout=5) as s:
        s.sendall(frame(msg))
        data = read_frame(s)
    m = parse(data)
    if 11 not in m:  # punch_hole_response
        print("unexpected-response fields=%s" % sorted(m))
        return 2
    resp = parse(m[11][0])
    code = resp.get(3, [0])[0]
    print(FAILURES.get(code, "FAILURE_%d" % code))
    return 0


def relay(host, key, port, hold=3.0):
    rr = field_str(2, "rsprobe-" + os.urandom(8).hex()) + field_str(6, key)
    msg = field_bytes(18, rr)  # RendezvousMessage.request_relay
    with socket.create_connection((host, port), timeout=5) as s:
        s.sendall(frame(msg))
        s.settimeout(hold)
        start = time.monotonic()
        try:
            data = s.recv(1)
        except socket.timeout:
            print("accepted")
            return 0
        except ConnectionResetError:
            data = b""
        if data == b"":
            print("refused after %.1fs" % (time.monotonic() - start))
            return 0
        print("unexpected-data")
        return 2


def tcp(host, port):
    try:
        with socket.create_connection((host, port), timeout=5):
            print("open")
    except ConnectionRefusedError:
        print("refused")
    except (socket.timeout, TimeoutError):
        print("filtered")
    except OSError:
        print("unreachable")
    return 0


def main(argv):
    if len(argv) < 3:
        print(__doc__, file=sys.stderr)
        return 2
    cmd, host = argv[1], argv[2]
    try:
        if cmd == "tcp" and len(argv) >= 4:
            return tcp(host, int(argv[3]))
        if cmd == "udp-register":
            return udp_register(host, int(argv[3]) if len(argv) > 3 else 21116)
        if cmd == "punch" and len(argv) >= 4:
            return punch(host, argv[3], int(argv[4]) if len(argv) > 4 else 21116)
        if cmd == "relay" and len(argv) >= 4:
            return relay(host, argv[3], int(argv[4]) if len(argv) > 4 else 21117)
    except (OSError, ValueError) as e:
        print("error: %s" % e)
        return 2
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
