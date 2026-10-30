# SPDX-License-Identifier: GPL-3.0-or-later
"""Softimage|3D picture files (.pic), reader only (our own code, from finding 0003 of the
repository).  decode(bytes) -> (width, height, planes) where planes maps 'R','G','B','A'
to bytearrays of width*height, file row 0 first.  decode_np(bytes) gives the same planes as
numpy uint8 arrays (numpy ships with Blender) and is what the engine uses: one Python step per
run header, the pixels copied in bulk.  `_decode_py` is the plain per-run reader; decode_np
falls back to it for anything it does not take (a damaged file gets that reader's error)."""
import struct

MAGIC = 0x5380F634
PLANES = ((0x80, "R"), (0x40, "G"), (0x20, "B"), (0x10, "A"))


class PicError(Exception):
    pass


def _decode_py(d):
    if len(d) < 108 or struct.unpack_from(">I", d, 0)[0] != MAGIC or d[88:92] != b"PICT":
        raise PicError("not a Softimage picture")
    width, height = struct.unpack_from(">hh", d, 92)
    if width <= 0 or height <= 0:
        raise PicError("invalid size")
    o = 104
    packets = []
    while True:
        chained, size, typ, channel = d[o:o + 4]
        o += 4
        packets.append((typ & 0xF, [n for bit, n in PLANES if channel & bit]))
        if not chained:
            break
    planes = {}
    for _, names in packets:
        for n in names:
            planes[n] = bytearray(width * height)
    n = len(d)
    for y in range(height):
        row = y * width
        for typ, names in packets:
            k = len(names)
            if typ == 0:                       # raw, planes interleaved
                if o + width * k > n:
                    raise PicError("file ends inside row %d" % y)
                for i, nm in enumerate(names):
                    planes[nm][row:row + width] = d[o + i:o + width * k:k]
                o += width * k
            elif typ == 2:                     # mixed run coding
                x = 0
                while x < width:
                    if o >= n:
                        raise PicError("file ends inside row %d" % y)
                    c = d[o]; o += 1
                    if c < 0x80:
                        c += 1
                        if x + c > width or o + c * k > n:
                            raise PicError("bad run in row %d" % y)
                        for i, nm in enumerate(names):
                            planes[nm][row + x:row + x + c] = d[o + i:o + c * k:k]
                        o += c * k
                    else:
                        if c == 0x80:
                            c, = struct.unpack_from(">H", d, o); o += 2
                        else:
                            c -= 0x7F
                        if x + c > width or o + k > n:
                            raise PicError("bad run in row %d" % y)
                        for i, nm in enumerate(names):
                            planes[nm][row + x:row + x + c] = bytes([d[o + i]]) * c
                        o += k
                    x += c
            # type 1 (pure run) carries nothing per row
    return width, height, planes


def _decode_fast(d):
    """decode_np's reader, or None for anything it does not take: then _decode_py decides (same result or error).

    Pass 1 (Python, one step per run header): walks the rows exactly as _decode_py does and marks every header
    of a mixed packet; a run that passes the end of its row, or a file that ends early, gives None.
    Pass 2 (numpy): from the headers, each run's pixel count and the offset of its bytes; then the file offset
    of every pixel's first byte (a cumulative sum: +k inside a list of pixels, +0 inside a repeated pixel, a jump
    at each run) and one gather per plane.  A plane carried by two packets keeps the later packet's bytes, as
    _decode_py's row-by-row writing leaves it; a plane only a type-1 (or unknown) packet names stays zero."""
    import numpy as np
    n = len(d)
    if n < 108 or bytes(d[0:4]) != b"\x53\x80\xf6\x34" or bytes(d[88:92]) != b"PICT":
        return None
    width, height = struct.unpack_from(">hh", d, 92)
    if width <= 0 or height <= 0:
        return None
    o = 104
    packets = []
    while True:
        if o + 4 > n:
            return None
        chained, typ, channel = d[o], d[o + 2], d[o + 3]
        o += 4
        names = [nm for bit, nm in PLANES if channel & bit]
        packets.append((typ & 0xF, names, len(names)))
        if not chained:
            break
    mask = bytearray(n)              # 1 at each run header of a mixed packet
    segs = []                        # per (row, mixed packet), in file order: offset of its first header
    raws = [[] for _ in packets]     # per raw packet: offset of each row's bytes
    try:
        for y in range(height):
            for j, (typ, names, k) in enumerate(packets):
                if typ == 2:
                    segs.append(o)
                    x = 0
                    while x < width:
                        c = d[o]
                        mask[o] = 1
                        if c < 0x80:
                            c += 1
                            o += 1 + c * k
                        elif c == 0x80:
                            c = (d[o + 1] << 8) | d[o + 2]
                            o += 3 + k
                        else:
                            c -= 0x7F
                            o += 1 + k
                        x += c
                    if x != width or o > n:
                        return None
                elif typ == 0:
                    raws[j].append(o)
                    o += width * k
                    if o > n:
                        return None
    except IndexError:
        return None
    data = np.frombuffer(d, np.uint8)
    npix = width * height
    mixed = [j for j, p in enumerate(packets) if p[0] == 2]
    hdr = np.flatnonzero(np.frombuffer(mask, np.uint8))
    if mixed:
        if len(mixed) == 1:
            per = {mixed[0]: hdr}
        else:
            first = np.searchsorted(hdr, np.asarray(segs, dtype=np.int64))
            counts = np.diff(np.append(first, len(hdr)))
            seg_packet = np.tile(np.asarray(mixed), height)
            run_packet = np.repeat(seg_packet, counts)
            per = {j: hdr[run_packet == j] for j in mixed}
    planes = {}
    for typ, names, k in packets:
        for nm in names:
            planes[nm] = None
    for j, (typ, names, k) in enumerate(packets):
        if k == 0 or typ not in (0, 2):
            continue
        if typ == 0:
            src = (np.asarray(raws[j], dtype=np.int64)[:, None] + np.arange(width, dtype=np.int64)[None, :] * k).ravel()
        else:
            h = per[j]
            c = data[h].astype(np.int64)
            lit = c < 0x80
            cnt = c - 0x7F
            cnt[lit] = c[lit] + 1
            off = h + 1
            lng = np.flatnonzero(c == 0x80)
            if len(lng):
                hl = h[lng]
                cnt[lng] = (data[hl + 1].astype(np.int64) << 8) | data[hl + 2]
                off[lng] += 2
            if int(cnt.sum()) != npix:
                return None
            keep = cnt > 0
            if not keep.all():
                cnt, off, lit = cnt[keep], off[keep], lit[keep]
            step = lit * k
            start = np.cumsum(cnt) - cnt                  # first pixel of each run
            delta = np.repeat(step, cnt)
            jump = off.copy()
            jump[1:] -= off[:-1] + step[:-1] * (cnt[:-1] - 1)
            delta[start] = jump
            src = np.cumsum(delta)
        if len(src) != npix:
            return None
        for i, nm in enumerate(names):
            planes[nm] = data[src + i] if i else data[src]
    for nm, v in planes.items():
        if v is None:
            planes[nm] = np.zeros(npix, np.uint8)
    return width, height, planes


def decode_np(d):
    """(width, height, planes): planes maps the plane names to numpy uint8 arrays of width*height, file row 0 first.
    The same values (and, for a file it cannot read, the same error) as decode."""
    import numpy as np
    try:
        r = _decode_fast(d)
    except Exception:
        r = None
    if r is not None:
        return r
    w, h, planes = _decode_py(d)
    return w, h, {k: np.frombuffer(v, np.uint8) for k, v in planes.items()}


def decode(d):
    try:
        w, h, planes = decode_np(d)
    except ImportError:                      # no numpy: the plain reader
        return _decode_py(d)
    return w, h, {k: bytearray(v.tobytes()) for k, v in planes.items()}


def decode_zpic(data, w, h, full_w=None, x0=0):
    """The depth file `<name>.<frame>.Zpic` (finding 0012 section 31): big-endian float32, no header, rows top to
    bottom as the picture's; -100000 where nothing was hit; camera-space z of the nearest surface (negative in
    front of the camera). Returns an array('f') of w*h values for the picture of w x h, or None.
    Whole frame: exactly w*h values. A picture with a render sub-region (the scene's own or Blender's border):
    the file's lines are the full frame width `full_w`, the region's rows come first (one extra line follows),
    the picture's columns are x0 .. x0+w-1 (measured on IK-PageTurn: 0 of 7138+ pixels differ in coverage)."""
    import array
    import sys
    a = array.array("f")
    if len(data) == w * h * 4:
        a.frombytes(data)
        if sys.byteorder == "little":
            a.byteswap()
        return a
    if full_w and full_w > w and len(data) % (full_w * 4) == 0 and len(data) // (full_w * 4) >= h and 0 <= x0 <= full_w - w:
        b = array.array("f")
        b.frombytes(data)
        if sys.byteorder == "little":
            b.byteswap()
        for y in range(h):
            a.extend(b[y * full_w + x0: y * full_w + x0 + w])
        return a
    return None


def fields_of(d):
    """The header's fields word: 3 whole frame, 1 and 2 one field of an interlaced frame."""
    return struct.unpack_from(">h", d, 100)[0]


def weave(a, b, first_rows_flag=1):
    """Two field pictures of one frame (same size, fields words 1 and 2: file bytes `a` and `b`) -> (width, 2 * height, planes);
    the field whose word is `first_rows_flag` fills rows 0, 2, 4 ... (counted from the top), the other rows 1, 3, 5 ...
    The planes are numpy uint8 arrays (as decode_np's)."""
    import numpy as np
    fa, fb = fields_of(a), fields_of(b)
    if {fa, fb} != {1, 2}:
        raise PicError("not the two fields of a frame")
    wa, ha, pa = decode_np(a)
    wb, hb, pb = decode_np(b)
    if (wa, ha) != (wb, hb):
        raise PicError("fields differ in size")
    top, bot = (pa, pb) if fa == first_rows_flag else (pb, pa)
    planes = {}
    for n in pa:
        out = np.empty((2 * ha, wa), np.uint8)
        out[0::2] = top[n].reshape(ha, wa)
        out[1::2] = bot[n].reshape(ha, wa)
        planes[n] = out.ravel()
    return wa, ha * 2, planes
