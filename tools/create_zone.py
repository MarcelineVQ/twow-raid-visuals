#!/usr/bin/env python3
import struct
import os
from pathlib import Path
import argparse
import sys

# Constants for M2 offsets
SEQ_ARRAY_META_OFF = 28    # u32 sequence count, u32 sequence array offset
TIMING_ARRAY_OFF   = 0xAE0 # subordinate timing array (5 × u32)
M2_VERTEX_STRIDE   = 48    # bytes per vertex
DEFAULT_ORIG_HALF  = 12.5  # original quad half-size for locating vertices
DEFAULT_INPUT      = "DangerZone.m2"
# Hardcoded offset of the texture string in this M2
TEXTURE_NAME_OFF   = 0xA90  # offset to 'SPELLS\\DANGERAREABLUE.BLP\x00'

# Binary read/write helpers

def read_u32(data, off): return struct.unpack_from("<I", data, off)[0]

def write_u32(data, off, val): struct.pack_into("<I", data, off, val)

def read_str(data, off):
    end = data.find(b'\x00', off)
    return data[off:end].decode('ascii', errors='ignore')

def write_bytes(data, off, bts):
    data[off:off+len(bts)] = bts

# Quad resizing

def resize_quad(data, diameter, orig_half):
    import struct as _s
    pat = _s.pack("<3f", -orig_half, -orig_half, 0.0)
    idx = data.find(pat)
    if idx == -1:
        raise RuntimeError(f"Quad vertices with half-size {orig_half} not found.")
    half = diameter / 2.0
    new_positions = [(-half, -half, 0.0), (half, -half, 0.0),
                     (-half, half, 0.0), (half, half, 0.0)]
    for i, pos in enumerate(new_positions):
        off = idx + i * M2_VERTEX_STRIDE
        data[off:off+12] = _s.pack("<3f", *pos)
    return idx, new_positions

# Timestamp patching

def patch_timestamps(data, fade_in, duration, fade_out):
    t0 = 0
    t1 = fade_in
    t2 = 1000
    t3 = t2 + duration
    t4 = t3 + fade_out
    times = [t0, t1, t2, t3, t4]
    for i, t in enumerate(times): write_u32(data, TIMING_ARRAY_OFF + 4*i, t)
    cnt = read_u32(data, SEQ_ARRAY_META_OFF)
    seq_ofs = read_u32(data, SEQ_ARRAY_META_OFF + 4)
    if cnt < 1 or seq_ofs == 0:
        raise RuntimeError("Sequence entry not found.")
    write_u32(data, seq_ofs + 4, t0)
    write_u32(data, seq_ofs + 8, t4)
    return times, seq_ofs

# Texture patching using fixed offset

def patch_texture(data, tex_base):
    if len(tex_base) > 20:
        raise RuntimeError("Texture name too long (max 20 chars)")
    full = f"SPELLS\\{tex_base.upper()}.BLP"
    new_bytes = full.encode('ascii') + b'\x00'
    old = read_str(data, TEXTURE_NAME_OFF)
    old_len = len(old) + 1
    # if len(new_bytes) > old_len:
        # raise RuntimeError(f"New texture '{full}' too long (max {old_len-1}).")
    write_bytes(data, TEXTURE_NAME_OFF, new_bytes)
    for p in range(TEXTURE_NAME_OFF + len(new_bytes), TEXTURE_NAME_OFF + old_len):
        data[p] = 0
    return old

# Main

def main():
    p = argparse.ArgumentParser(description="Resize quad, patch keyframes, and swap texture by base name.")
    p.add_argument("input", nargs='?', default=DEFAULT_INPUT, help=f"Input M2 file (default {DEFAULT_INPUT})")
    p.add_argument("-o", "--output", nargs='?', const='',
                   help="Output file or directory (if directory, uses default filename there)")
    p.add_argument("--diameter", "-d", type=float, required=True, help="Quad diameter")
    p.add_argument("--fade-in", type=int, default=500, help="Fade-in ms")
    p.add_argument("--duration", type=int, default=8000, help="Hold duration ms")
    p.add_argument("--fade-out", type=int, default=1000, help="Fade-out ms")
    p.add_argument("--texture", "-t", help="Base texture name (up to 20 chars)")
    p.add_argument("--orig-half", type=float, default=DEFAULT_ORIG_HALF,
                   help="Original half-size for quad")
    args = p.parse_args()

    in_path = Path(args.input)
    if not in_path.exists():
        print(f"Input '{in_path}' not found.", file=sys.stderr)
        sys.exit(1)
    data = bytearray(in_path.read_bytes())

    idx, pos = resize_quad(data, args.diameter, args.orig_half)
    print(f"Resized quad at 0x{idx:X} to diameter {args.diameter}: {pos}")

    times, seq_ofs = patch_timestamps(data, args.fade_in, args.duration, args.fade_out)
    sec = args.duration // 1000
    print(f"Patched keyframes: {times}, sequence at 0x{seq_ofs:X}")

    if args.texture:
        old = patch_texture(data, args.texture)
        print(f"Replaced texture '{old}' with 'SPELLS\\{args.texture.upper()}.BLP' at 0x{TEXTURE_NAME_OFF:X}")

    # Determine default filename
    default_name = f"{in_path.stem}_W{int(args.diameter)}_S{sec}.m2"
    # Handle output
    if args.output is None:
        out_path = in_path.with_name(default_name)
    elif args.output == '':  # '-o' with no argument
        out_path = in_path.with_name(default_name)
    else:
        out = Path(args.output)
        if out.is_dir() or args.output.endswith(os.sep):
            out_path = out / default_name
        else:
            out_path = out
    out_path.write_bytes(data)
    print("Wrote:", out_path)

if __name__ == '__main__':
    main()
