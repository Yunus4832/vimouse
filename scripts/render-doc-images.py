#!/usr/bin/env python3
"""Export documentation PNGs from the HUD renderer; Python stdlib only.

No desktop connection, system input capture, or image background is involved.
"""
import os
from pathlib import Path
import struct
import subprocess
import tempfile
import zlib

ROOT = Path(__file__).resolve().parents[1]


def png_chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))


def export(source, destination):
    header, pixels = source.read_bytes().split(b"ENDHDR\n", 1)
    fields = dict(line.split(maxsplit=1) for line in header.splitlines()[1:])
    width, height = int(fields[b"WIDTH"]), int(fields[b"HEIGHT"])
    assert fields[b"TUPLTYPE"] == b"RGB_ALPHA" and fields[b"DEPTH"] == b"4"
    assert fields[b"MAXVAL"] == b"255" and len(pixels) == width * height * 4
    occupied = [i for i, alpha in enumerate(pixels[3::4]) if alpha]
    assert occupied, f"Empty fixture: {source}"
    padding = 16
    left = max(0, min(i % width for i in occupied) - padding)
    right = min(width, max(i % width for i in occupied) + padding + 1)
    top = max(0, occupied[0] // width - padding)
    bottom = min(height, occupied[-1] // width + padding + 1)
    rows = [pixels[(y * width + left) * 4:(y * width + right) * 4] for y in range(top, bottom)]
    alpha = b"".join(rows)[3::4]
    assert 0 in alpha and any(0 < value < 255 for value in alpha), "Missing transparency"
    image = b"\x89PNG\r\n\x1a\n"
    image += png_chunk(b"IHDR", struct.pack(">IIBBBBB", right - left, bottom - top, 8, 6, 0, 0, 0))
    image += png_chunk(b"IDAT", zlib.compress(b"".join(b"\0" + row for row in rows), 9))
    image += png_chunk(b"IEND", b"")
    destination.write_bytes(image)
    print(f"Exported {destination.relative_to(ROOT)}: {right - left}x{bottom - top}, RGBA")


def main():
    with tempfile.TemporaryDirectory(prefix="vimouse-doc-images-") as directory:
        subprocess.run(
            ["cargo", "test", "--locked", "--offline", "--lib", "hud::render::fixtures::render_all_tools", "--", "--exact"],
            cwd=ROOT,
            env=dict(os.environ, VIMOUSE_RENDER_DIR=directory),
            check=True,
        )
        for fixture, name in [("normal", "hud"), ("command-typed", "command-hud"), ("help", "help")]:
            export(Path(directory) / f"{fixture}.pam", ROOT / "docs" / f"{name}.png")


if __name__ == "__main__":
    main()
