#!/usr/bin/env python3
"""协议级联调自测：模拟主程序 UDP 服务行为，验证 rcu-simulator 的发现/下发链路。"""
import socket, struct, sys, time

FIND_HEX = "0002000100220010FE0000144B696E67696E745263758F933D6D0000000000000D0A"

def parse_header(b):
    return {
        "serial": struct.unpack(">H", b[0:2])[0],
        "device_type": struct.unpack(">H", b[2:4])[0],
        "data_len": struct.unpack(">H", b[4:6])[0],
        "func": b[7],
        "reg": struct.unpack(">H", b[8:10])[0],
        "reg_num": struct.unpack(">H", b[10:12])[0],
    }

def main():
    # 1. 模拟主程序绑定 4668（顺延起点）
    tool = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    tool.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    tool.bind(("0.0.0.0", 4668))
    tool.settimeout(3)

    # 2. 发送发现广播到 4667（本机回环）
    tool.sendto(bytes.fromhex(FIND_HEX), ("127.0.0.1", 4667))
    data, src = tool.recvfrom(4096)
    h = parse_header(data)
    assert h["reg"] == 0xFE01, f"期望 0xFE01 应答，实际 0x{h['reg']:04X}"
    payload = data[12:12 + h["data_len"]]
    if payload[-2:] == b"\r\n":
        payload = payload[:-2]
    assert len(payload) >= 121, f"载荷长度不足: {len(payload)}"
    equip = payload[0:8].hex().upper()
    model = struct.unpack(">H", payload[8:10])[0]
    hotel = struct.unpack(">I", payload[16:20])[0]
    room = payload[22]
    ip = ".".join(str(x) for x in payload[25:29])
    print(f"[发现] equipId={equip} model=0x{model:04X} hotel={hotel} room={room} ip={ip}")
    assert equip == "AABBCCDD11223344"

    # 3. 下发基础信息(0xFE12)：hotel=2002 build=2 floor=8 room=808%256=48? 用 room=48 door=3
    appid = bytes.fromhex("54a750a0d76d34d8ddfe81a5cf81c74e")
    base = appid + struct.pack(">I", 2002) + bytes([2, 8, 48, 3])
    pkt = struct.pack(">HHHBBHH", 1, 1, 0, 0, 0x10, 0xFE12, 100) + base + b"\r\n"
    tool.sendto(pkt, ("127.0.0.1", 4699))
    data, _ = tool.recvfrom(4096)
    h = parse_header(data)
    payload = data[12:12 + h["data_len"]]
    if payload[-2:] == b"\r\n":
        payload = payload[:-2]
    hotel2 = struct.unpack(">I", payload[16:20])[0]
    build2, floor2, room2, door2 = payload[20], payload[21], payload[22], payload[23]
    print(f"[基础信息回读] hotel={hotel2} build={build2} floor={floor2} room={room2} door={door2}")
    assert (hotel2, build2, floor2, room2, door2) == (2002, 2, 8, 48, 3)

    # 4. 下发网络配置(0xFE10)：静态 192.168.134.99
    cfg = bytes([0x00]) \
        + bytes([192, 168, 134, 99]) + bytes([255, 255, 255, 0]) + bytes([192, 168, 134, 1]) \
        + b"\xff" * 6 + bytes([0x00]) + bytes(32) \
        + bytes([192, 168, 134, 1]) + struct.pack(">H", 9999) + bytes([223, 5, 5, 5])
    pkt = struct.pack(">HHHBBHH", 1, 1, 0, 0, 0x10, 0xFE10, 100) + cfg + b"\r\n"
    tool.sendto(pkt, ("127.0.0.1", 4699))
    data, _ = tool.recvfrom(4096)
    h = parse_header(data)
    payload = data[12:12 + h["data_len"]]
    if payload[-2:] == b"\r\n":
        payload = payload[:-2]
    ip2 = ".".join(str(x) for x in payload[25:29])
    port2 = struct.unpack(">H", payload[80:82])[0]
    print(f"[网络配置回读] ip={ip2} serverPort={port2}")
    assert ip2 == "192.168.134.99" and port2 == 9999

    # 5. 固件升级全流程：下发 0x02F2 首指令 → 应答 0x02F3 拉包请求 → 0x02F4 分包应答
    uid, num, seg_size = 0x1234, 3, 4
    total = num * seg_size
    md5 = "0123456789ABCDEF0123456789ABCDEF"
    # "0000"+hotelFlag(X2)+v0(X2)+v2(X4)+v1(X2)+updateTime(X8)+fileId(X4)+total(X8)+num(X4)+md5
    first = (
        "0000" + "01" + "01" + "0108" + "02"
        + "6789ABCD"
        + f"{uid:04X}" + f"{total:08X}" + f"{num:04X}" + md5
    )
    body = bytes.fromhex(first).ljust(40, b"\x00")
    pkt = struct.pack(">HHHBBHH", 40, 1, 54, 0, 0x10, 0x02F2, 40) + body + b"\r\n"
    tool.sendto(pkt, ("127.0.0.1", 4699))

    seg_md5 = "D41D8CD98F00B204E9800998ECF8427E"
    for expect_idx in range(1, num + 1):
        data, dev_addr = tool.recvfrom(4096)
        h = parse_header(data)
        assert h["reg"] == 0x02F3, f"期望拉包请求 0x02F3，实际 0x{h['reg']:04X}"
        req_payload = data[12:]
        if req_payload[-2:] == b"\r\n":
            req_payload = req_payload[:-2]
        assert len(req_payload) >= 14, f"拉包请求载荷过短: {len(req_payload)}"
        req_uid = struct.unpack(">H", req_payload[10:12])[0]
        req_idx = struct.unpack(">H", req_payload[12:14])[0]
        print(f"[升级] 收到拉包请求 uid=0x{req_uid:04X} index={req_idx}")
        assert req_uid == uid and req_idx == expect_idx, "拉包请求序号乱序"
        # 分包应答：uid(X4)+index(X4)+len(X4)+hex数据+md5
        seg_hex = (f"A{expect_idx}B{expect_idx}C{expect_idx}D{expect_idx}")
        resp = f"{uid:04X}{expect_idx:04X}{seg_size:04X}{seg_hex}{seg_md5}"
        resp_body = bytes.fromhex(resp).ljust(26 + seg_size, b"\x00")
        reg_num = 26 + seg_size
        pkt = struct.pack(">HHHBBHH", reg_num, 1, reg_num + 14, 0, 0x10, 0x02F4, reg_num) \
            + resp_body + b"\r\n"
        tool.sendto(pkt, dev_addr)
    print(f"[升级] {num} 个分包全部应答完毕，模拟器应已完成升级")

    print("ALL PASS ✔ 发现/基础信息/网络配置/固件升级 协议链路验证通过")

if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        print(f"FAIL: {e}")
        sys.exit(1)
