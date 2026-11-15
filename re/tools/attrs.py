#!/usr/bin/env python3
"""Extract CAttributeStore registrations (id, type, default, min, max) from decompiled FUN_140076950."""
import re, struct, sys
exe = open(sys.argv[1], 'rb').read()
src = open(sys.argv[2]).read()
SECS = [(0x1000, 0xe3000, 0x400), (0xe4000, 0x9a800, 0xe3400), (0x17f000, 0x59600, 0x17dc00)]
def dbl(name):
    if not name.startswith('DAT_'):
        return name
    va = int(name[4:], 16) - 0x140000000
    for v, sz, raw in SECS:
        if v <= va < v + sz:
            return struct.unpack_from('<d', exe, va - v + raw)[0]
    return name
start = src.index('// ==== 140076950')
body = src[start:src.index('// ==== ', start + 10)]
env = {}
typ = None; dflt = None
for line in body.splitlines():
    line = line.strip()
    m = re.match(r'(dVar\d+) = (DAT_\w+);', line)
    if m: env[m.group(1)] = dbl(m.group(2))
    m = re.search(r'local_f8 = (CAttributeValue<[^>]+>)', line)
    if m: typ = m.group(1)
    m = re.match(r'local_e8 = \(double\)CONCAT44\(local_e8\._4_4_,(\w+)\);', line)
    if m:
        try: dflt = int(m.group(1), 0)
        except ValueError: dflt = m.group(1)
    m = re.match(r'local_e8 = (.+);', line)
    if m and 'CONCAT44' not in line: dflt = m.group(1)
    m = re.search(r'CAttributeStore::Add\(this,(\w+),pCVar\d+,?(.*)\);', line)
    if m:
        rest = [a.strip() for a in m.group(2).split(',') if a.strip()]
        rng = [env.get(a, dbl(a)) for a in rest]
        print(int(m.group(1), 0), typ, dflt, rng)
