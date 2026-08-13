#!/usr/bin/env python3
"""诊断工具：独立端口(4770)发送 0xFE00 发现广播，打印设备响应原始 HEX。
用于核对 RCU 协议载荷布局（新旧固件差异）。不影响主服务(4668)。"""
import socket
import time

FIND = ("255.255.255.255", 4667)
PKT = bytes.fromhex(
    "0002000100220010FE0000144B696E67696E745263758F933D6D0000000000000D0A"
)

s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
s.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("0.0.0.0", 4770))
s.settimeout(1.0)

print("listening on :4770, broadcasting discovery every 3s for 12s...", flush=True)
end = time.time() + 12
last_send = 0.0
while time.time() < end:
    if time.time() - last_send >= 3:
        s.sendto(PKT, FIND)
        last_send = time.time()
    try:
        data, addr = s.recvfrom(4096)
    except socket.timeout:
        continue
    reg = data[8:10].hex().upper() if len(data) >= 12 else "??"
    fc = data[7:8].hex().upper() if len(data) >= 8 else "??"
    print(f"\n=== from {addr[0]}:{addr[1]} len={len(data)} func={fc} reg={reg} ===", flush=True)
    print(data.hex().upper(), flush=True)
print("\ndone.", flush=True)
