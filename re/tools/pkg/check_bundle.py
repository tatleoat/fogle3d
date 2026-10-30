#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Bundle check (unit pkg1, release model): build the add-on zip + engine binaries into a staging directory and FAIL
if anything from the user's Softimage|3D 4.0 install is inside it.

  python3 -I re/tools/pkg/check_bundle.py [--no-build] [--bin-dir DIR] [--out DIR] [--install DIR]

Stages  <out>/stage/fogle3d/   the add-on (blender-addon/softimage3d without __pycache__)
        <out>/stage/fogle3d/bin/{render-frames,blend2db,si-import-dump}   our compiled programs (si-import-dump:
        the .dsc importer's reader, re/tools/import1, so File > Import works from the zip alone)
and writes <out>/fogle3d-addon.zip (default out: re/out/pkg; untracked). Checks, on the staged files AND on every
member of the zip:
  1. names: no Softimage asset name or extension (the inventory, docs/install-requirements.md);
  2. hashes: no staged file / zip member has the sha256 of any file of the install (the install is walked once; only
     files whose size equals a staged file's size are hashed; contents are never printed);
  3. embedding: no staged file contains a 64-byte window of the key assets (Relations.rsrc, noIcon.pic, the template
     set-up), sampled every 256 bytes of the asset;
  4. the original program's data and messages: the release audit's bundle category (re/tools/release/release.py
     audit_tree(..., bundle_only=True), as make_release.sh --bundle runs it) over the staged files: no 64-byte window of
     the Linux soft's data sections and no block of its messages (16 or more in a file) compiled into our programs.
     The engine reads those from the user's install at run time (crates/si-rt); the counts per file are printed
     (<out>/stage.audit.tsv has them with their details).
Prints PASS/FAIL lines and the staged path only. Without an install to compare with (none given, SI_INSTALL unset, no
development copy) checks 2 and 3 are "not compared" and the script fails unless --allow-no-install.
The licence file is never touched. Our own code."""
import argparse
import hashlib
import os
import shutil
import subprocess
import sys
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
DEV_DEFAULT = os.path.join(ROOT, "re", "bin", "si3d40-ia", "installed")

# names and extensions of what the install holds (docs/install-requirements.md); none may be staged
FORBIDDEN_NAMES = {"Relations.rsrc", "noIcon.pic", "MODEL-Gears.1-0.sts", "Chapter.rsrc", "soft", "Setup", "cdinst.dat"}
FORBIDDEN_EXT = {".rsrc", ".pic", ".dsc", ".sts", ".hrc", ".mtr", ".ani", ".lig", ".cam", ".chn", ".shp", ".t3d", ".shd",
                 ".cls", ".exp", ".lat", ".spd", ".wav", ".flk", ".iso", ".so", ".dll", ".exe", ".ttf", ".pfb", ".fnt", ".ico"}
KEY_ASSETS = ("Relations.rsrc", "noIcon.pic", "MODEL-Gears.1-0.sts")
PROGRAMS = ("render-frames", "blend2db", "si-import-dump")


def sha256_file(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def build(target):
    env = dict(os.environ, CARGO_TARGET_DIR=target)
    cmds = [["cargo", "build", "--release", "--offline", "--manifest-path", "re/tools/si-render/pictures/Cargo.toml", "--bin", "render-frames"],
            ["cargo", "build", "--release", "--offline", "--manifest-path", "re/tools/builder2/blend2db/Cargo.toml"],
            ["cargo", "build", "--release", "--offline", "--manifest-path", "re/tools/import1/Cargo.toml"]]
    for c in cmds:
        r = subprocess.run(["/bin/sh", "-c", 'ulimit -v 8000000; exec "$@"', "sh"] + c, cwd=ROOT, env=env)
        if r.returncode:
            sys.exit("build failed: " + " ".join(c))


def stage(out, bindir):
    st = os.path.join(out, "stage")
    if os.path.isdir(st):
        shutil.rmtree(st)                   # our own staging directory, a fixed path under --out
    dst = os.path.join(st, "fogle3d")
    shutil.copytree(os.path.join(ROOT, "blender-addon", "softimage3d"), dst, ignore=shutil.ignore_patterns("__pycache__", "*.pyc", "bin"))
    os.makedirs(os.path.join(dst, "bin"))
    for p in PROGRAMS:
        src = os.path.join(bindir, p)
        if not os.path.isfile(src):
            sys.exit("missing program: " + src)
        shutil.copy2(src, os.path.join(dst, "bin", p))
        os.chmod(os.path.join(dst, "bin", p), 0o755)
    zp = os.path.join(out, "fogle3d-addon.zip")
    with zipfile.ZipFile(zp, "w", zipfile.ZIP_DEFLATED) as z:
        for r, _, fs in sorted(os.walk(dst)):
            for f in sorted(fs):
                full = os.path.join(r, f)
                zi = zipfile.ZipInfo.from_file(full, os.path.relpath(full, dst))
                zi.compress_type = zipfile.ZIP_DEFLATED
                with open(full, "rb") as fh:
                    z.writestr(zi, fh.read())
    return st, zp


def staged_items(st, zp):
    """(label, name, size, loader) for every staged file and every zip member."""
    items = []
    for r, _, fs in os.walk(st):
        for f in fs:
            p = os.path.join(r, f)
            items.append(("stage:" + os.path.relpath(p, st), f, os.path.getsize(p), (lambda p=p: open(p, "rb").read())))
    z = zipfile.ZipFile(zp)
    for i in z.infolist():
        if not i.is_dir():
            items.append(("zip:" + i.filename, os.path.basename(i.filename), i.file_size, (lambda n=i.filename: z.read(n))))
    return items


def program_data(st, inst):
    """Check 4: the release audit's bundle category over the staged tree; returns the failures (one per hit)."""
    sys.path.insert(0, os.path.join(ROOT, "re", "tools", "release"))
    import release                      # our own audit (re/tools/release/release.py)
    print("---- 4. the release audit's bundle category (data windows and message blocks of the original program)")
    release.audit_tree(st, st, inst, quick=True, bundle_only=True)
    hits = []
    tsv = st.rstrip(os.sep) + ".audit.tsv"
    if os.path.isfile(tsv):
        for line in open(tsv, encoding="utf-8"):
            if line.startswith("6 "):
                cat, path, _, excerpt, detail = (line.rstrip("\n").split("\t") + [""] * 5)[:5]
                hits.append((path, "%s (%s)" % (excerpt or detail, cat[2:].split(" inside")[0])))
    for path, detail in hits:
        print("  %s: %s" % (path, detail))
    print("%s program data: %d staged files hold the original's data windows or a block of its messages"
          % ("PASS" if not hits else "FAIL", len({h[0] for h in hits})))
    return ["program data: %s (%s)" % h for h in hits]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--no-build", action="store_true")
    ap.add_argument("--bin-dir", help="directory with render-frames, blend2db and si-import-dump (default <CARGO_TARGET_DIR>/release)")
    ap.add_argument("--out", default=os.path.join(ROOT, "re", "out", "pkg"))
    ap.add_argument("--install")
    ap.add_argument("--allow-no-install", action="store_true")
    a = ap.parse_args()
    target = os.environ.get("CARGO_TARGET_DIR") or os.path.join(ROOT, "target", "pkg")
    if not a.no_build and not a.bin_dir:
        build(target)
    bindir = a.bin_dir or os.path.join(target, "release")
    os.makedirs(a.out, exist_ok=True)
    st, zp = stage(a.out, bindir)
    items = staged_items(st, zp)
    fails = []

    # 0. identity (owner, 2026-10-29): the extension is Fogle3D, id fogle3d, engine FOGLE3D; no direct .dsc render in the UI
    mf = open(os.path.join(st, "fogle3d", "blender_manifest.toml"), encoding="utf-8").read()
    ini = open(os.path.join(st, "fogle3d", "__init__.py"), encoding="utf-8").read()
    ident = [("manifest id", 'id = "fogle3d"' in mf), ("manifest name", 'name = "Fogle3D"' in mf),
             ("notice in manifest", "unofficial" in mf.lower() and "not affiliated" in mf.lower()),
             ("engine id", 'ENGINE_ID = "FOGLE3D"' in ini and 'bl_label = "Fogle3D"' in ini),
             ("no scene picker in the UI", "scene_filter" not in ini and 'name="Source"' not in ini)]
    for what, ok in ident:
        print("%s identity: %s" % ("PASS" if ok else "FAIL", what))
        if not ok:
            fails.append("identity: " + what)

    # 1. names
    bad = [lab for lab, name, _, _ in items if name in FORBIDDEN_NAMES or os.path.splitext(name)[1].lower() in FORBIDDEN_EXT]
    print("%s names: %d staged files and zip members, %d with a Softimage asset name or extension" % ("PASS" if not bad else "FAIL", len(items), len(bad)))
    fails += bad

    # 2/3. against the install
    inst = a.install or os.environ.get("SI_INSTALL") or (DEV_DEFAULT if os.path.isdir(DEV_DEFAULT) else "")
    if not inst or not os.path.isdir(inst):
        print("NOT-COMPARED hashes and embedding: no install to compare with")
        if not a.allow_no_install:
            fails.append("no install to compare with")
    else:
        sizes = {}
        for lab, _, size, _ in items:
            sizes.setdefault(size, []).append(lab)
        staged_hash = {}
        for lab, _, size, load in items:
            staged_hash[lab] = hashlib.sha256(load()).hexdigest()
        by_hash = {}
        for lab, h in staged_hash.items():
            by_hash.setdefault(h, []).append(lab)
        walked = hashed = 0
        hits = []
        assets = {}
        for r, _, fs in os.walk(inst):
            for f in fs:
                p = os.path.join(r, f)
                if os.path.islink(p):
                    continue
                walked += 1
                try:
                    sz = os.path.getsize(p)
                except OSError:
                    continue
                if sz in sizes and sz > 0:
                    hashed += 1
                    h = sha256_file(p)
                    if h in by_hash:
                        hits += by_hash[h]
                if f in KEY_ASSETS and f not in assets:
                    assets[f] = p
        print("%s hashes: %d install files walked, %d hashed (size equal to a staged file), %d staged files identical to an install file"
              % ("PASS" if not hits else "FAIL", walked, hashed, len(hits)))
        fails += hits
        found = 0
        windows = 0
        blobs = [load() for _, _, _, load in items]
        for name, p in assets.items():
            data = open(p, "rb").read()
            for off in range(0, max(0, len(data) - 64), 256):
                w = data[off:off + 64]
                if len(set(w)) < 16:          # padding and repeats are not evidence
                    continue
                windows += 1
                if any(w in b for b in blobs):
                    found += 1
        print("%s embedding: %d windows of %d key assets looked for in %d staged files, %d found" % ("PASS" if not found else "FAIL", windows, len(assets), len(blobs), found))
        if found:
            fails.append("embedded asset bytes")
        missing = [k for k in KEY_ASSETS if k not in assets]
        if missing:
            print("NOTE key assets not in this install (embedding not compared for them): " + ", ".join(missing))
        fails += program_data(st, inst)
    print("staged: %s" % st)
    print("zip: %s (%d bytes)" % (zp, os.path.getsize(zp)))
    print("RESULT %s" % ("FAIL" if fails else "PASS"))
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
