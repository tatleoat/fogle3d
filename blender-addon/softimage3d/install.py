# SPDX-License-Identifier: GPL-3.0-or-later
"""Where the user's own Softimage|3D 4.0 install is, and whether it holds what the add-on's programs read.

Release model (owner, 2026-10-29): nothing of Softimage ships with the add-on or the engine; every asset is read from
the user's install at run time. This is the one place in the add-on that resolves the install root; its Rust twin is
re/tools/pkg/si-install (same rules, same texts). The inventory is docs/install-requirements.md.

Root, in order: the scene's "Install dir" field (default: env SI3D_INSTALLED, kept for the tests), the environment
variable SI_INSTALL, the add-on preference "Softimage 3D 4.0 install path". Our own code.
"""
import glob
import os

ENV = "SI_INSTALL"
PREF_LABEL = "Softimage 3D 4.0 install path"

RENDER = "render"
BLEND2DB = "blend2db"

# what, file name, needed by (the long version with purposes: docs/install-requirements.md). BLEND2DB here is the
# "This Blender scene" source, which runs render-frames after blend2db: it needs the program file too.
ITEMS = (
    ("relations resource file", "Relations.rsrc", (RENDER, BLEND2DB)),
    ("default picture", "noIcon.pic", (RENDER, BLEND2DB)),
    ("template render set-up", "MODEL-Gears.1-0.sts", (BLEND2DB,)),
    ("program file of the Linux build", "Creative-Tools/3D/bin/soft", (RENDER, BLEND2DB)),
)

# The program file the engine reads the original's own tables and messages from at run time (crates/si-rt: the same
# names, length and places). Only its name and size are looked at here; render-frames checks its checksum when it
# starts and exits with status 2 and the reason when it is not the expected build.
PROGRAM_NAME = "soft"
PROGRAM_LEN = 8472833
PROGRAM_PATHS = ("Creative-Tools/3D/bin/soft", "linux/soft3d/Creative-Tools/3D/bin/soft", "soft3d/Creative-Tools/3D/bin/soft")


_FOUND = {}     # (root, name, prefer) -> what find() returned last, while that file is still there


def find(root, name, prefer=""):
    """First path named `name` below `root` (sorted; a path holding `prefer` first).  The walk over the install (17,000
    directories, 0.07 s) ran three times per render; a hit is remembered while its file exists (unit speed2)."""
    key = (root, name, prefer)
    hit = _FOUND.get(key)
    if hit is not None and os.path.isfile(hit):
        return hit
    hits = sorted(glob.glob(os.path.join(glob.escape(root), "**", name), recursive=True))
    pref = [h for h in hits if prefer and prefer in h]
    hit = (pref or hits or [None])[0]
    if hit is not None:
        _FOUND[key] = hit
    return hit


def find_program(root):
    """(path, None) for the program file of the Linux build under `root` (the usual places first, else a search by
    name and size, a path holding /linux/ first), or (None, why) with the readable reason: absent, or only files of
    another size (another platform's build)."""
    key = (root, PROGRAM_NAME, "program")
    hit = _FOUND.get(key)
    if hit is not None and os.path.isfile(hit) and os.path.getsize(hit) == PROGRAM_LEN:
        return hit, None
    others = []
    for rel in PROGRAM_PATHS:
        p = os.path.join(root, rel)
        if os.path.isfile(p):
            if os.path.getsize(p) == PROGRAM_LEN:
                _FOUND[key] = p
                return p, None
            others.append(p)
    hits = sorted(glob.glob(os.path.join(glob.escape(root), "**", PROGRAM_NAME), recursive=True))
    hits.sort(key=lambda h: "/linux/" not in h)
    for h in hits:
        if os.path.isfile(h) and not os.path.islink(h):
            if os.path.getsize(h) == PROGRAM_LEN:
                _FOUND[key] = h
                return h, None
            others.append(h)
    if others:
        return None, ("%s is not the Softimage|3D 4.0 Linux build Fogle3D was made for (%d bytes, expected %d)"
                      % (others[0], os.path.getsize(others[0]), PROGRAM_LEN))
    return None, ""


def program_env(root):
    """The environment our programs need beside their arguments: the install root in SI_INSTALL, where render-frames
    and si-import-dump find the program file when their argument is not the install (the "This Blender scene"
    source's temporary database, a scene overlay)."""
    return {ENV: root} if root else {}


def resolve_root(scene_value="", pref_value="", abspath=lambda p: p):
    """The root as the user set it ("" when nothing is set anywhere)."""
    for v in (scene_value, os.environ.get(ENV, ""), pref_value):
        if v:
            return abspath(v)
    return ""


def not_set_text():
    return ("The %s is not set. Set it in Preferences > Add-ons > Fogle3D > \"%s\" (the directory your own "
            "Softimage|3D 4.0 was installed to), or in the environment variable %s." % (PREF_LABEL, PREF_LABEL, ENV))


def check(root, need=RENDER, program_root=None):
    """(info, problems): info is {root, rsrc, rsrc_dir, no_icon, template_sts, program} when the install holds what `need`
    reads, problems is a list of readable lines otherwise (what is missing, by name). `program_root`: where the program
    file is looked for when `root` is not the install (a database blend2db built, which holds copies of the resource
    files but not the program file); default `root`."""
    if not root:
        return None, [not_set_text()]
    if not os.path.isdir(root):
        return None, ["The %s is not a directory: %s" % (PREF_LABEL, root)]
    miss = []
    direct = os.path.join(root, "Creative-Rsrc", "3D", "rsrc", "Relations.rsrc")
    rsrc = direct if os.path.isfile(direct) else find(root, "Relations.rsrc", "/linux/")
    info = {"root": root, "rsrc": rsrc, "rsrc_dir": None, "no_icon": None, "template_sts": None, "program": None}
    if rsrc is None:
        miss += [ITEMS[0][:2], ITEMS[1][:2]]
    else:
        info["rsrc_dir"] = os.path.dirname(rsrc)
        info["no_icon"] = os.path.join(info["rsrc_dir"], "noIcon.pic")
        if not os.path.isfile(info["no_icon"]):
            miss.append(ITEMS[1][:2])
    if need == BLEND2DB:
        info["template_sts"] = find(root, "MODEL-Gears.1-0.sts")
        if info["template_sts"] is None:
            miss.append(ITEMS[2][:2])
    info["program"], why = find_program(program_root or root)
    if info["program"] is None:
        miss.append((ITEMS[3][0], ITEMS[3][1] + (": " + why if why else "")))
    if miss:
        lines = ["%s does not look like a Softimage|3D 4.0 install; missing:" % root]
        lines += ["  - %s (%s)" % m for m in miss]
        lines.append("Point the \"%s\" (or %s) at the directory the product was installed to, the one that holds "
                     "Creative-Rsrc/" % (PREF_LABEL, ENV))
        return None, lines
    return info, []
