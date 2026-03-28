#!/usr/bin/env python3
"""Convert PNG to BLP2 (DXT3 compressed, with mipmaps)."""

import struct
import sys
from PIL import Image


def rgb_to_565(r, g, b):
    return ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)


def color_565_to_rgb(c):
    r = ((c >> 11) & 0x1F) << 3
    g = ((c >> 5) & 0x3F) << 2
    b = (c & 0x1F) << 3
    return r, g, b


def color_distance(r0, g0, b0, r1, g1, b1):
    return (r0 - r1) ** 2 + (g0 - g1) ** 2 + (b0 - b1) ** 2


def encode_dxt1_block(pixels):
    """Encode a 4x4 block of (r,g,b) tuples into 8-byte DXT1 color block.
    pixels: list of 16 (r,g,b) tuples in row-major order.
    """
    # Find min/max colors by luminance
    min_c = min(pixels, key=lambda c: c[0] * 299 + c[1] * 587 + c[2] * 114)
    max_c = max(pixels, key=lambda c: c[0] * 299 + c[1] * 587 + c[2] * 114)

    color0 = rgb_to_565(*max_c)
    color1 = rgb_to_565(*min_c)

    # Ensure color0 > color1 for 4-color mode
    if color0 == color1:
        # All same color, indices all 0
        return struct.pack('<HHI', color0, color1, 0)

    if color0 < color1:
        color0, color1 = color1, color0
        max_c, min_c = min_c, max_c

    # Reconstruct actual RGB values after 565 quantization
    r0, g0, b0 = color_565_to_rgb(color0)
    r1, g1, b1 = color_565_to_rgb(color1)

    # Build 4-color palette
    palette = [
        (r0, g0, b0),
        (r1, g1, b1),
        ((2 * r0 + r1) // 3, (2 * g0 + g1) // 3, (2 * b0 + b1) // 3),
        ((r0 + 2 * r1) // 3, (g0 + 2 * g1) // 3, (b0 + 2 * b1) // 3),
    ]

    # Find best index for each pixel
    indices = 0
    for i, (r, g, b) in enumerate(pixels):
        best_idx = 0
        best_dist = float('inf')
        for j, (pr, pg, pb) in enumerate(palette):
            d = color_distance(r, g, b, pr, pg, pb)
            if d < best_dist:
                best_dist = d
                best_idx = j
        indices |= best_idx << (i * 2)

    return struct.pack('<HHI', color0, color1, indices)


def encode_dxt3_block(pixels):
    """Encode a 4x4 block of (r,g,b,a) tuples into 16-byte DXT3 block.
    pixels: list of 16 (r,g,b,a) tuples in row-major order.
    """
    # Alpha: 4 bits per pixel, 64 bits total
    alpha_bits = 0
    for i, (_, _, _, a) in enumerate(pixels):
        alpha_bits |= (a >> 4) << (i * 4)

    alpha_data = struct.pack('<Q', alpha_bits)

    # Color: DXT1 block (ignore alpha)
    rgb_pixels = [(r, g, b) for r, g, b, _ in pixels]
    color_data = encode_dxt1_block(rgb_pixels)

    return alpha_data + color_data


def get_block_pixels(img_data, width, height, bx, by):
    """Extract 4x4 pixel block from RGBA image data."""
    pixels = []
    for py in range(4):
        for px in range(4):
            x = bx * 4 + px
            y = by * 4 + py
            if x < width and y < height:
                idx = (y * width + x) * 4
                pixels.append(tuple(img_data[idx:idx + 4]))
            else:
                pixels.append((0, 0, 0, 0))
    return pixels


def encode_dxt3_image(img_data, width, height):
    """Encode full RGBA image as DXT3."""
    blocks_wide = (width + 3) // 4
    blocks_high = (height + 3) // 4
    result = bytearray()

    for by in range(blocks_high):
        for bx in range(blocks_wide):
            pixels = get_block_pixels(img_data, width, height, bx, by)
            result.extend(encode_dxt3_block(pixels))

    return bytes(result)


def generate_mipmaps(img):
    """Generate mipmap chain from PIL Image. Returns list of (width, height, rgba_bytes)."""
    levels = [img]
    w, h = img.size
    while w > 1 or h > 1:
        w = max(1, w // 2)
        h = max(1, h // 2)
        levels.append(img.resize((w, h), Image.LANCZOS))
    return [(l.size[0], l.size[1], l.tobytes()) for l in levels]


def png_to_blp2(png_path, blp_path):
    img = Image.open(png_path).convert('RGBA')
    width, height = img.size

    mipmaps = generate_mipmaps(img)
    num_mipmaps = min(len(mipmaps), 16)

    # Encode each mipmap level as DXT3
    encoded = []
    for w, h, data in mipmaps[:num_mipmaps]:
        encoded.append(encode_dxt3_image(data, w, h))

    # BLP2 header: 1172 bytes
    # 4 (magic) + 4 (type) + 1 (compression) + 1 (alpha_depth) + 1 (alpha_type)
    # + 1 (has_mips) + 4 (width) + 4 (height) + 64 (offsets) + 64 (lengths)
    # + 1024 (palette) = 1172
    header_size = 4 + 4 + 1 + 1 + 1 + 1 + 4 + 4 + 64 + 64 + 1024

    # Calculate offsets
    offsets = [0] * 16
    lengths = [0] * 16
    offset = header_size
    for i, data in enumerate(encoded):
        offsets[i] = offset
        lengths[i] = len(data)
        offset += len(data)

    # Build header
    header = bytearray()
    header.extend(b'BLP2')
    header.extend(struct.pack('<I', 1))       # type = 1 (BLP/DXTC/Uncompressed)
    header.extend(struct.pack('<B', 2))       # compression = 2 (DXTC)
    header.extend(struct.pack('<B', 8))       # alpha_depth = 8
    header.extend(struct.pack('<B', 1))       # alpha_type = 1 (DXT3)
    header.extend(struct.pack('<B', 1))       # has_mips = 1
    header.extend(struct.pack('<I', width))
    header.extend(struct.pack('<I', height))
    for o in offsets:
        header.extend(struct.pack('<I', o))
    for l in lengths:
        header.extend(struct.pack('<I', l))
    header.extend(b'\x00' * 1024)             # palette (unused for DXTC)

    assert len(header) == header_size

    with open(blp_path, 'wb') as f:
        f.write(header)
        for data in encoded:
            f.write(data)

    print(f'{png_path} -> {blp_path}')
    print(f'  {width}x{height}, {num_mipmaps} mipmaps, DXT3')
    print(f'  {offset} bytes total')


if __name__ == '__main__':
    if len(sys.argv) != 3:
        print(f'Usage: {sys.argv[0]} input.png output.blp')
        sys.exit(1)
    png_to_blp2(sys.argv[1], sys.argv[2])
