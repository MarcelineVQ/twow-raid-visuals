#!/usr/bin/env python3
import struct, os
from pathlib import Path
import argparse, sys

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
def read_str(data, off): end = data.find(b'\x00', off); return data[off:end].decode('ascii', errors='ignore')
def write_bytes(data, off, bts): data[off:off+len(bts)] = bts

# Resize quad vertices
def resize_quad(data, diameter, orig_half):
    import struct as _s
    pat = _s.pack("<3f", -orig_half, -orig_half, 0.0)
    idx = data.find(pat)
    if idx < 0:
        raise RuntimeError(f"Quad vertices with half-size {orig_half} not found.")
    half = diameter / 2.0
    positions = [
        (-half, -half, 0.0), ( half, -half, 0.0),
        (-half,  half, 0.0), ( half,  half, 0.0),
    ]
    for i, pos in enumerate(positions):
        off = idx + i * M2_VERTEX_STRIDE
        data[off:off+12] = _s.pack("<3f", *pos)
    return idx, positions

# Patch full manual times
def patch_full_times(data, times):
    if len(times) != 5:
        raise ValueError("--times-full requires exactly 5 values")
    for i, t in enumerate(times): write_u32(data, TIMING_ARRAY_OFF + 4*i, t)
    cnt = read_u32(data, SEQ_ARRAY_META_OFF)
    seq_ofs = read_u32(data, SEQ_ARRAY_META_OFF + 4)
    if cnt < 1 or seq_ofs == 0:
        raise RuntimeError("Sequence entry not found.")
    write_u32(data, seq_ofs + 4, times[0])
    write_u32(data, seq_ofs + 8, times[-1])
    duration_ms = times[3] - times[2]
    return times, seq_ofs, duration_ms

# Patch incremental mode
def patch_incremental(data, fade_in, duration, fade_out):
    t0, t1, t2 = 0, fade_in, 1000
    t3 = t2 + duration; t4 = t3 + fade_out
    times = [t0, t1, t2, t3, t4]
    for i, t in enumerate(times): write_u32(data, TIMING_ARRAY_OFF + 4*i, t)
    cnt = read_u32(data, SEQ_ARRAY_META_OFF)
    seq_ofs = read_u32(data, SEQ_ARRAY_META_OFF + 4)
    if cnt < 1 or seq_ofs == 0:
        raise RuntimeError("Sequence entry not found.")
    write_u32(data, seq_ofs + 4, t0)
    write_u32(data, seq_ofs + 8, t4)
    return times, seq_ofs, duration

# Patch texture string
def patch_texture(data, tex_base):
    if len(tex_base) > 20:
        raise RuntimeError("Texture name too long (max 20 chars)")
    full = f"SPELLS\\{tex_base.upper()}.BLP"
    new_bytes = full.encode('ascii') + b'\x00'
    old = read_str(data, TEXTURE_NAME_OFF)
    old_len = len(old) + 1
    write_bytes(data, TEXTURE_NAME_OFF, new_bytes)
    for p in range(TEXTURE_NAME_OFF + len(new_bytes), TEXTURE_NAME_OFF + old_len):
        data[p] = 0
    return old

# Argument parsing
def parse_args():
    p = argparse.ArgumentParser(description="Resize quad, patch keyframes, and swap texture.")
    p.add_argument("input", nargs='?', default=DEFAULT_INPUT, help=f"Input M2 (default {DEFAULT_INPUT})")
    p.add_argument("-o","--output", nargs='?', const='', help="Output file or dir")
    p.add_argument("-d","--diameter",type=float, required=True, help="Quad diameter")
    group = p.add_mutually_exclusive_group()
    group.add_argument("--times-full", help="5 comma-separated keyframe times (ms)")
    group.add_argument("--fade-in", type=int, default=500, help="Fade-in ms")
    p.add_argument("--duration", type=int, default=8000, help="Hold duration ms")
    p.add_argument("--fade-out", type=int, default=1000, help="Fade-out ms")
    p.add_argument("-t","--texture", help="Base texture name (up to 20 chars)")
    p.add_argument("--orig-half", type=float, default=DEFAULT_ORIG_HALF, help="Original half-size of quad")
    return p.parse_args()

# Main
def main():
    args = parse_args()
    path = Path(args.input)
    if not path.exists():
        print(f"Input '{path}' not found.", file=sys.stderr); sys.exit(1)
    data = bytearray(path.read_bytes())

    idx, positions = resize_quad(data, args.diameter, args.orig_half)
    print(f"Resized quad @0x{idx:X} to diameter {args.diameter}: {positions}")

    if args.times_full:
        times = [int(x) for x in args.times_full.split(",")]
        times, seq_ofs, duration_ms = patch_full_times(data, times)
        print(f"Patched full keyframes: {times}, seq @0x{seq_ofs:X}")
    else:
        times, seq_ofs, duration_ms = patch_incremental(data, args.fade_in, args.duration, args.fade_out)
        print(f"Patched keyframes: {times}, seq @0x{seq_ofs:X}")

    sec = duration_ms // 1000

    if args.texture:
        old = patch_texture(data, args.texture)
        print(f"Replaced texture '{old}' -> 'SPELLS\\{args.texture.upper()}.BLP' @0x{TEXTURE_NAME_OFF:X}")

    default_name = f"{path.stem}_W{int(args.diameter)}_S{sec}.m2"
    if args.output is None or args.output == '':
        out_path = path.with_name(default_name)
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
