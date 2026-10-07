#!/usr/bin/env python3
"""Write a small animated GIF (a dot circling a ring) with no image library.
Uses "uncompressed" LZW: literal codes with a clear code every 250 pixels."""
import math, struct, sys

W = H = 96
FRAMES = 12
PALETTE = [(255, 255, 255), (47, 111, 235), (230, 230, 236), (220, 60, 60)]


def lzw(indices, min_code=8):
    clear, eoi = 1 << min_code, (1 << min_code) + 1
    codes, n = [clear], 0
    for i in indices:
        if n == 250:
            codes.append(clear)
            n = 0
        codes.append(i)
        n += 1
    codes.append(eoi)
    out, acc, bits = bytearray(), 0, 0
    for c in codes:
        acc |= c << bits
        bits += min_code + 1
        while bits >= 8:
            out.append(acc & 0xFF)
            acc >>= 8
            bits -= 8
    if bits:
        out.append(acc & 0xFF)
    blocks = bytearray([min_code])
    for k in range(0, len(out), 255):
        chunk = out[k:k + 255]
        blocks += bytes([len(chunk)]) + chunk
    return bytes(blocks + b"\x00")


def frame(t):
    a = 2 * math.pi * t / FRAMES
    cx, cy = W / 2 + 30 * math.cos(a), H / 2 + 30 * math.sin(a)
    px = []
    for y in range(H):
        for x in range(W):
            r = math.hypot(x - W / 2, y - H / 2)
            if (x - cx) ** 2 + (y - cy) ** 2 < 100:
                px.append(3)
            elif 28 < r < 32:
                px.append(2)
            else:
                px.append(0 if (x // 12 + y // 12) % 2 else 0)
    return px


def main(path):
    g = bytearray(b"GIF89a")
    g += struct.pack("<HHBBB", W, H, 0x91, 0, 0)
    for rgb in PALETTE:
        g += bytes(rgb)
    g += b"\x21\xFF\x0BNETSCAPE2.0\x03\x01\x00\x00\x00"
    for t in range(FRAMES):
        g += b"\x21\xF9\x04\x00" + struct.pack("<H", 8) + b"\x00\x00"
        g += b"\x2C" + struct.pack("<HHHH", 0, 0, W, H) + b"\x00"
        g += lzw(frame(t))
    g += b"\x3B"
    open(path, "wb").write(g)


if __name__ == "__main__":
    main(sys.argv[1])
