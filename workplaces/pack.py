#!/usr/bin/env python3
"""Packs a workplace folder into a .tar.gz whose bytes depend only on the folder's files.

    workplaces/pack.py workplaces/counter counter-1.0.0.tar.gz

Prints `<size> <sha256>` of the archive: the two numbers a catalog entry pins. The same
folder gives the same bytes on every machine, so the entry is computed locally and the
release job checks it: members in the order of their paths, no owner, time zero, mode 755 for
what runs and 644 for the rest, and the gzip layer holds "stored" blocks, which no compressor
version can change. Not packed: .DS_Store and __pycache__.
"""
import hashlib
import io
import os
import struct
import sys
import tarfile
import zlib


def pack(folder: str) -> bytes:
    tar = io.BytesIO()
    with tarfile.open(fileobj=tar, mode="w", format=tarfile.USTAR_FORMAT) as t:
        for root, dirs, files in os.walk(folder):
            dirs[:] = sorted(d for d in dirs if d != "__pycache__")
            for name in dirs + sorted(f for f in files if f != ".DS_Store"):
                path = os.path.join(root, name)
                info = t.gettarinfo(path, os.path.relpath(path, folder))
                if not (info.isfile() or info.isdir()):
                    sys.exit(f"{path}: only files and folders can be packed")
                info.mtime = info.uid = info.gid = 0
                info.uname = info.gname = ""
                info.mode = 0o755 if info.isdir() or info.mode & 0o111 else 0o644
                t.addfile(info, open(path, "rb") if info.isfile() else None)
    data = tar.getvalue()
    # gzip header without a name or time; deflate blocks of the kind "stored", at most 64 KiB.
    out = bytearray(b"\x1f\x8b\x08\x00\x00\x00\x00\x00\x00\xff")
    for i in range(0, len(data), 65535):
        chunk = data[i : i + 65535]
        out += struct.pack("<BHH", i + 65535 >= len(data), len(chunk), len(chunk) ^ 0xFFFF) + chunk
    return bytes(out + struct.pack("<II", zlib.crc32(data), len(data)))


if __name__ == "__main__":
    if len(sys.argv) != 3 or not os.path.isdir(sys.argv[1]):
        sys.exit("usage: pack.py <workplace folder> <out.tar.gz>")
    archive = pack(sys.argv[1])
    with open(sys.argv[2], "wb") as f:
        f.write(archive)
    print(len(archive), hashlib.sha256(archive).hexdigest())
