#!/usr/bin/env python3
"""Turn a real .tachibk backup into a committable test fixture.

Walks the protobuf wire format directly rather than through a schema, so fields
Kani does not model (Suwayomi's 101/9000/9001 blocks) survive byte-identical —
a fixture re-encoded through our own .proto would only test our own
understanding of the format.

Personal values (titles, urls, authors, descriptions, cover urls, chapter names,
scanlators, category and source names) are replaced with synthetic ones of the
same shape. Numbers, field order, wire types and presence/absence are untouched.

    python3 scripts/anonymise-tachibk.py in.tachibk out.tachibk --manga 4 --chapters 5
"""

import argparse
import gzip
import hashlib
import re


def read_varint(buf, i):
    shift = 0
    val = 0
    while True:
        byte = buf[i]
        i += 1
        val |= (byte & 0x7F) << shift
        if not byte & 0x80:
            return val, i
        shift += 7


def write_varint(val):
    out = bytearray()
    while True:
        byte = val & 0x7F
        val >>= 7
        if val:
            out.append(byte | 0x80)
        else:
            out.append(byte)
            return bytes(out)


def parse(buf):
    i = 0
    out = []
    while i < len(buf):
        key, i = read_varint(buf, i)
        num, wt = key >> 3, key & 7
        if wt == 0:
            val, i = read_varint(buf, i)
        elif wt == 1:
            val, i = buf[i:i + 8], i + 8
        elif wt == 2:
            ln, i = read_varint(buf, i)
            val, i = buf[i:i + ln], i + ln
        elif wt == 5:
            val, i = buf[i:i + 4], i + 4
        else:
            raise ValueError(f"unsupported wire type {wt} at offset {i}")
        out.append((num, wt, val))
    return out


def serialise(fields):
    out = bytearray()
    for num, wt, val in fields:
        out += write_varint((num << 3) | wt)
        if wt == 0:
            out += write_varint(val)
        elif wt == 2:
            out += write_varint(len(val))
            out += val
        else:
            out += val
    return bytes(out)


def tag(value, salt):
    return hashlib.blake2b(bytes(value) + salt.encode(), digest_size=6).hexdigest()


def fake_uuid(value):
    h = tag(value, "uuid")
    return f"{h[:8]}-{h[:4]}-4{h[1:4]}-8{h[2:5]}-{h}{h[:4]}"


SERIES = ["Tidewalker", "Lantern Street", "Salt and Cinder", "The Quiet Ward",
          "Paper Cranes", "Nine Roofs", "Harbour Bell", "Iron Camellia"]
PEOPLE = ["Aoki Rei", "Mizuno Haru", "Tanabe Kou", "Sera Michi"]
GROUPS = ["Driftwood Scans", "Lamplight TL", "Blue Kettle Group"]
CATEGORIES = ["Reading", "Backlog", "Finished"]
SOURCES = ["Example Source", "Second Source", "Third Source"]

DESCRIPTION = (
    "A synthetic description standing in for the original. It is long enough to "
    "exercise the same length class as a real one without carrying any of its text."
)


def pick(pool, value, salt):
    idx = int(tag(value, salt), 16) % len(pool)
    return pool[idx]


def anonymise_chapter(fields, manga_key):
    out = []
    for num, wt, val in fields:
        if num == 1:
            out.append((num, wt, f"/chapter/{fake_uuid(val)}".encode()))
        elif num == 2:
            n = int(tag(val, "chno"), 16) % 400
            out.append((num, wt, f"Ch. {n} - {pick(SERIES, val, 'chtitle')}".encode()))
        elif num == 3:
            out.append((num, wt, pick(GROUPS, val, "group").encode()))
        else:
            out.append((num, wt, val))
    return out


def anonymise_history(fields):
    return [
        (num, wt, f"/chapter/{fake_uuid(val)}".encode()) if num == 1 else (num, wt, val)
        for num, wt, val in fields
    ]


def anonymise_tracking(fields):
    out = []
    for num, wt, val in fields:
        if num == 4:
            out.append((num, wt, b"https://tracker.example.com/manga/12345"))
        elif num == 5:
            out.append((num, wt, pick(SERIES, val, "trktitle").encode()))
        else:
            out.append((num, wt, val))
    return out


def anonymise_manga(fields, chapter_limit, index):
    key = next((val for num, _, val in fields if num == 2), b"")
    title = SERIES[index % len(SERIES)]
    kept_chapters = 0
    out = []
    for num, wt, val in fields:
        if num == 2:
            out.append((num, wt, f"/manga/{fake_uuid(val)}".encode()))
        elif num == 3:
            out.append((num, wt, title.encode()))
        elif num in (4, 5):
            out.append((num, wt, pick(PEOPLE, val, f"person{num}").encode()))
        elif num == 6:
            out.append((num, wt, DESCRIPTION.encode()))
        elif num == 9:
            out.append((num, wt, f"https://covers.example.com/{fake_uuid(val)}.jpg".encode()))
        elif num == 16:
            if chapter_limit is not None and kept_chapters >= chapter_limit:
                continue
            kept_chapters += 1
            out.append((num, wt, serialise(anonymise_chapter(parse(val), key))))
        elif num == 18:
            out.append((num, wt, serialise(anonymise_tracking(parse(val)))))
        elif num == 104:
            if chapter_limit is not None and kept_chapters > chapter_limit:
                continue
            out.append((num, wt, serialise(anonymise_history(parse(val)))))
        else:
            out.append((num, wt, val))
    return out


HOSTILE_TITLES = [
    "../../../../etc/passwd",
    "..\\..\\windows\\system32\\drivers",
    "nul\x00inside",
    "‮gnp.esrever",
    "𠜎𠜱 astral pair",
]


def make_hostile(fields, index):
    title = HOSTILE_TITLES[index % len(HOSTILE_TITLES)]
    return [
        (num, wt, title.encode()) if num == 3 else (num, wt, val)
        for num, wt, val in fields
    ]


def augment(fields):
    tracking = serialise([
        (1, 0, 2),
        (2, 0, 0),
        (4, 2, b"https://tracker.example.com/manga/12345"),
        (5, 2, SERIES[0].encode()),
        (6, 5, b"\x00\x00\x60\x41"),
        (7, 0, 40),
        (8, 5, b"\x00\x00\x0c\x42"),
        (9, 0, 1),
        (100, 0, 12345),
    ])
    out = list(fields)
    out.append((17, 0, 0))
    out.append((18, 2, tracking))
    return out


HOSTISH = re.compile(rb"://|\d{1,3}(\.\d{1,3}){3}|@")
CREDENTIALISH = re.compile(rb"^(?!\d+$)[A-Za-z][\w.-]{2,}$")


def redact_settings(fields, block):
    out = []
    for num, wt, val in fields:
        if wt != 2 or not val:
            out.append((num, wt, val))
        elif HOSTISH.search(val):
            out.append((num, wt, b"http://redacted.invalid:8191"))
        elif block == 9001 and CREDENTIALISH.match(val):
            out.append((num, wt, b"redacted"))
        else:
            out.append((num, wt, val))
    return out


def anonymise_backup(data, manga_limit, chapter_limit, augment_first=False, hostile=False):
    kept_manga = 0
    out = []
    for num, wt, val in parse(data):
        if num == 1:
            if manga_limit is not None and kept_manga >= manga_limit:
                continue
            kept_manga += 1
            fields = anonymise_manga(parse(val), chapter_limit, kept_manga - 1)
            if hostile:
                fields = make_hostile(fields, kept_manga - 1)
            if augment_first and kept_manga == 1:
                fields = augment(fields)
            out.append((num, wt, serialise(fields)))
        elif num == 2:
            inner = [
                (n, w, pick(CATEGORIES, v, "cat").encode()) if n == 1 else (n, w, v)
                for n, w, v in parse(val)
            ]
            out.append((num, wt, serialise(inner)))
        elif num == 101:
            inner = [
                (n, w, pick(SOURCES, v, "source").encode()) if n == 1 else (n, w, v)
                for n, w, v in parse(val)
            ]
            out.append((num, wt, serialise(inner)))
        elif num in (9000, 9001):
            out.append((num, wt, serialise(redact_settings(parse(val), num))))
        else:
            out.append((num, wt, val))
    return serialise(out)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("source")
    ap.add_argument("dest")
    ap.add_argument("--manga", type=int, default=None, help="keep only the first N series")
    ap.add_argument("--chapters", type=int, default=None, help="keep only the first N chapters per series")
    ap.add_argument(
        "--augment-first",
        action="store_true",
        help="add a category assignment and a tracking entry to the first series, "
        "for donor backups that contain neither",
    )
    ap.add_argument(
        "--hostile-titles",
        action="store_true",
        help="replace every title with a path-traversal / NUL / RTL-override / astral case",
    )
    args = ap.parse_args()

    with gzip.open(args.source, "rb") as fh:
        raw = fh.read()
    result = anonymise_backup(raw, args.manga, args.chapters, args.augment_first, args.hostile_titles)
    with open(args.dest, "wb") as raw_fh:
        with gzip.GzipFile(fileobj=raw_fh, mode="wb", mtime=0) as fh:
            fh.write(result)
    print(f"{len(raw)} bytes in, {len(result)} bytes out -> {args.dest}")


if __name__ == "__main__":
    main()
