#!/usr/bin/env python3
"""Minimal PE32+ parser: dumps imports, version info and resources of mspaint.exe.

Usage: python3 -I pe_dump.py <exe> <outdir>
Writes imports.txt, resources.txt, strings_table.txt, menus.txt, accelerators.txt,
dialogs.txt and raw resource blobs under <outdir>/res/.
"""
import os
import struct
import sys

RT = {1: "CURSOR", 2: "BITMAP", 3: "ICON", 4: "MENU", 5: "DIALOG", 6: "STRING",
      7: "FONTDIR", 8: "FONT", 9: "ACCELERATOR", 10: "RCDATA", 11: "MESSAGETABLE",
      12: "GROUP_CURSOR", 14: "GROUP_ICON", 16: "VERSION", 23: "HTML", 24: "MANIFEST"}


class PE:
    def __init__(self, path):
        self.d = open(path, "rb").read()
        d = self.d
        pe = struct.unpack_from("<I", d, 0x3C)[0]
        assert d[pe:pe + 4] == b"PE\0\0"
        nsec, = struct.unpack_from("<H", d, pe + 6)
        optsz, = struct.unpack_from("<H", d, pe + 20)
        opt = pe + 24
        magic, = struct.unpack_from("<H", d, opt)
        self.is64 = magic == 0x20B
        self.imagebase = struct.unpack_from("<Q" if self.is64 else "<I", d, opt + (24 if self.is64 else 28))[0]
        ddoff = opt + (112 if self.is64 else 96)
        self.dirs = [struct.unpack_from("<II", d, ddoff + 8 * i) for i in range(16)]
        self.secs = []
        so = opt + optsz
        for i in range(nsec):
            name = d[so:so + 8].rstrip(b"\0").decode()
            vsz, va, rsz, rptr = struct.unpack_from("<IIII", d, so + 8)
            self.secs.append((name, va, vsz, rptr, rsz))
            so += 40

    def off(self, rva):
        for name, va, vsz, rptr, rsz in self.secs:
            if va <= rva < va + max(vsz, rsz):
                return rva - va + rptr
        raise ValueError(hex(rva))

    def cstr(self, rva):
        o = self.off(rva)
        return self.d[o:self.d.index(b"\0", o)].decode("latin1")

    def imports(self):
        out = {}
        rva, size = self.dirs[1]
        o = self.off(rva)
        while True:
            ilt, _, _, name, iat = struct.unpack_from("<IIIII", self.d, o)
            if name == 0:
                break
            dll = self.cstr(name)
            funcs = []
            p = self.off(ilt or iat)
            while True:
                v = struct.unpack_from("<Q", self.d, p)[0]
                if v == 0:
                    break
                if v >> 63:
                    funcs.append("#%d" % (v & 0xFFFF))
                else:
                    funcs.append(self.cstr((v & 0x7FFFFFFF) + 2))
                p += 8
            out[dll] = funcs
            o += 20
        # delay imports
        rva, size = self.dirs[13]
        if rva:
            o = self.off(rva)
            while True:
                attrs, name, hmod, iat, int_, *_ = struct.unpack_from("<IIIIIIII", self.d, o)
                if name == 0:
                    break
                dll = self.cstr(name) + " (delay)"
                funcs = []
                p = self.off(int_)
                while True:
                    v = struct.unpack_from("<Q", self.d, p)[0]
                    if v == 0:
                        break
                    funcs.append("#%d" % (v & 0xFFFF) if v >> 63 else self.cstr((v & 0x7FFFFFFF) + 2))
                    p += 8
                out[dll] = funcs
                o += 32
        return out

    def resources(self):
        rva, size = self.dirs[2]
        base = self.off(rva)
        res = []

        def name_of(entry_name):
            if entry_name & 0x80000000:
                o = base + (entry_name & 0x7FFFFFFF)
                n, = struct.unpack_from("<H", self.d, o)
                return self.d[o + 2:o + 2 + 2 * n].decode("utf-16le")
            return entry_name

        def walk(o, path):
            _, _, _, _, nn, ni = struct.unpack_from("<IIHHHH", self.d, base + o)
            for i in range(nn + ni):
                nm, tgt = struct.unpack_from("<II", self.d, base + o + 16 + 8 * i)
                p = path + [name_of(nm)]
                if tgt & 0x80000000:
                    walk(tgt & 0x7FFFFFFF, p)
                else:
                    drva, dsz = struct.unpack_from("<II", self.d, base + tgt)
                    do = self.off(drva)
                    res.append((p, self.d[do:do + dsz]))
        walk(0, [])
        return res


def u16z(b, o):
    s = []
    while True:
        c, = struct.unpack_from("<H", b, o)
        o += 2
        if c == 0:
            return "".join(s), o
        s.append(chr(c))


def parse_menu(b):
    lines = []
    ver, hdr = struct.unpack_from("<HH", b, 0)
    o = 4 + hdr
    if ver == 1:  # MENUEX
        def items(o, depth):
            while True:
                typ, state, mid, flags = struct.unpack_from("<IIIH", b, o)
                o += 14
                text, o = u16z(b, o)
                o = (o + 3) & ~3
                lines.append("  " * depth + ("---" if typ & 0x800 else "%s  [id=%d]" % (text, mid)))
                if flags & 1:
                    o += 4  # helpid
                    o = items(o, depth + 1)
                if flags & 0x80:
                    return o
        items(o, 0)
    else:
        def items(o, depth):
            while True:
                flags, = struct.unpack_from("<H", b, o)
                o += 2
                if flags & 0x10:
                    text, o = u16z(b, o)
                    lines.append("  " * depth + text)
                    o = items(o, depth + 1)
                else:
                    mid, = struct.unpack_from("<H", b, o)
                    o += 2
                    text, o = u16z(b, o)
                    lines.append("  " * depth + ("---" if not text and mid == 0 else "%s  [id=%d]" % (text, mid)))
                if flags & 0x80:
                    return o
        items(4, 0)
    return lines


VK = {8: "Backspace", 9: "Tab", 13: "Enter", 27: "Esc", 32: "Space", 33: "PgUp", 34: "PgDn", 35: "End",
      36: "Home", 37: "Left", 38: "Up", 39: "Right", 40: "Down", 45: "Insert", 46: "Delete",
      0x6B: "Num+", 0x6D: "Num-", 0xBB: "=", 0xBD: "-", 0xDB: "[", 0xDD: "]", 0xBC: ",", 0xBE: "."}


def parse_accel(b):
    out = []
    for o in range(0, len(b), 8):
        f, key, cmd, _ = struct.unpack_from("<HHHH", b, o)
        mods = []
        if f & 0x08: mods.append("Ctrl")
        if f & 0x10: mods.append("Alt")
        if f & 0x04: mods.append("Shift")
        if f & 0x01:
            k = VK.get(key) or ("F%d" % (key - 0x6F) if 0x70 <= key <= 0x87 else chr(key) if 32 < key < 127 else hex(key))
        else:
            k = repr(chr(key))
        out.append("%-24s -> cmd %d" % ("+".join(mods + [k]), cmd))
    return out


def parse_dialog(b):
    """Only DLGTEMPLATEEX is handled; returns control texts and ids."""
    out = []
    if struct.unpack_from("<HH", b, 0) != (1, 0xFFFF):
        return ["(classic DLGTEMPLATE, not parsed)"]
    o = 16
    ncontrols, = struct.unpack_from("<H", b, 0 + 16)
    x, y, cx, cy = struct.unpack_from("<hhhh", b, 18)
    o = 26

    def sz_or_ord(o):
        c, = struct.unpack_from("<H", b, o)
        if c == 0xFFFF:
            return "#%d" % struct.unpack_from("<H", b, o + 2)[0], o + 4
        if c == 0:
            return "", o + 2
        return u16z(b, o)
    menu, o = sz_or_ord(o)
    cls, o = sz_or_ord(o)
    title, o = u16z(b, o)
    style, = struct.unpack_from("<I", b, 12)
    out.append("title=%r size=%dx%d controls=%d" % (title, cx, cy, ncontrols))
    if style & 0x40:  # DS_SETFONT
        o += 6
        font, o = u16z(b, o)
    for i in range(ncontrols):
        o = (o + 3) & ~3
        _, _, _, cx_, cy_, = 0, 0, 0, 0, 0
        help_, exs, st, x, y, cx_, cy_, cid = struct.unpack_from("<IIIhhhhI", b, o)
        o += 24
        cls, o = sz_or_ord(o)
        text, o = sz_or_ord(o)
        extra, = struct.unpack_from("<H", b, o)
        o += 2 + extra
        clsname = {"#128": "BUTTON", "#129": "EDIT", "#130": "STATIC", "#131": "LISTBOX", "#132": "SCROLLBAR", "#133": "COMBOBOX"}.get(cls, cls)
        out.append("  %-10s id=%-6d %r" % (clsname, cid, text))
    return out


def main():
    exe, outdir = sys.argv[1], sys.argv[2]
    os.makedirs(os.path.join(outdir, "res"), exist_ok=True)
    pe = PE(exe)
    with open(os.path.join(outdir, "sections.txt"), "w") as f:
        f.write("imagebase=%#x\n" % pe.imagebase)
        for s in pe.secs:
            f.write("%-8s va=%#x vsz=%#x raw=%#x rsz=%#x\n" % s)
    with open(os.path.join(outdir, "imports.txt"), "w") as f:
        for dll, funcs in pe.imports().items():
            f.write("%s (%d)\n" % (dll, len(funcs)))
            for fn in funcs:
                f.write("    %s\n" % fn)
    res = pe.resources()
    with open(os.path.join(outdir, "resources.txt"), "w") as f:
        for p, data in res:
            t = RT.get(p[0], p[0])
            f.write("%-14s %-30s lang=%s size=%d\n" % (t, p[1], p[2] if len(p) > 2 else "", len(data)))
    strings, menus, accels, dialogs = [], [], [], []
    for p, data in res:
        t = RT.get(p[0], p[0])
        safe = "_".join(str(x) for x in [t] + p[1:])
        open(os.path.join(outdir, "res", safe + ".bin"), "wb").write(data)
        if t == "STRING":
            o = 0
            for i in range(16):
                n, = struct.unpack_from("<H", data, o)
                o += 2
                if n:
                    strings.append(((p[1] - 1) * 16 + i, data[o:o + 2 * n].decode("utf-16le")))
                o += 2 * n
        elif t == "MENU":
            menus.append("MENU %s\n" % p[1] + "\n".join(parse_menu(data)))
        elif t == "ACCELERATOR":
            accels.append("ACCEL %s\n" % p[1] + "\n".join(parse_accel(data)))
        elif t == "DIALOG":
            try:
                dialogs.append("DIALOG %s\n" % p[1] + "\n".join(parse_dialog(data)))
            except Exception as e:  # noqa
                dialogs.append("DIALOG %s (parse error %s)" % (p[1], e))
    with open(os.path.join(outdir, "strings_table.txt"), "w") as f:
        for i, s in sorted(strings):
            f.write("%6d  %s\n" % (i, s.replace("\n", "\\n")))
    open(os.path.join(outdir, "menus.txt"), "w").write("\n\n".join(menus))
    open(os.path.join(outdir, "accelerators.txt"), "w").write("\n\n".join(accels))
    open(os.path.join(outdir, "dialogs.txt"), "w").write("\n\n".join(dialogs))
    print("resources:", len(res), "strings:", len(strings), "menus:", len(menus), "accels:", len(accels), "dialogs:", len(dialogs))


if __name__ == "__main__":
    main()
