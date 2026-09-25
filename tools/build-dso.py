#!/usr/bin/env python3
"""Build data/dso.tsv: the Messier and Caldwell objects for the chart.

Run once by hand when the list should change; the chart reads the file
it writes and never goes online. Sources, all free to use:

- NASA HEASARC's Messier table (public domain): kind, magnitude, size.
- SIMBAD (CDS, Strasbourg): positions, sizes and tilt of each object.
- Wikipedia's Messier and Caldwell tables: which NGC/IC object each
  catalog number is, common names, and Caldwell magnitudes. Only these
  facts are taken, no text.

Output columns, tab-separated:
  id  alt  name  kind  ra_deg  dec_deg  mag  major'  minor'  pa_deg  constellation
"""
import html
import re
import socket
import sys
import urllib.parse
import urllib.request

UA = {"User-Agent": "fe2o3-starmap-build/0.1 (g@isene.com)"}

# IPv4 only: many networks (a phone hotspot, a hotel) do not route IPv6,
# and Python does not fall back the way curl does.
_gai = socket.getaddrinfo
socket.getaddrinfo = lambda h, p, f=0, *a, **k: _gai(h, p, socket.AF_INET, *a, **k)


def get(url, data=None):
    body = urllib.parse.urlencode(data).encode() if data else None
    req = urllib.request.Request(url, data=body, headers=UA)
    with urllib.request.urlopen(req, timeout=120) as r:
        return r.read().decode("utf-8", "replace")


def wiki(title):
    return get("https://en.wikipedia.org/w/index.php?title=%s&action=raw" % urllib.parse.quote(title))


def unlink(s):
    """Wiki markup to plain text: [[a|b]] -> b, templates and refs gone."""
    s = re.sub(r"<ref[^>]*/>", "", s)
    s = re.sub(r"<ref[^>]*>.*?</ref>", "", s)
    s = re.sub(r"\{\{(?:sdash|dsv|hs|wbr|shy)[^}]*\}\}", "", s)
    s = re.sub(r"\[\[File:[^\]]*\]\]", "", s)
    s = re.sub(r"\[\[(?:[^|\]]*\|)?([^\]]*)\]\]", r"\1", s)
    s = s.replace("''", "")
    return html.unescape(re.sub(r"\s+", " ", s)).strip()


def rows(table):
    """Split a wikitable into rows of cell strings."""
    out = []
    for block in table.split("\n|-")[1:]:
        cells = []
        for line in block.split("\n")[1:]:
            if line.startswith("|") or line.startswith("!"):
                # "! scope=row | x" and "| style=... | x" keep the last part
                cell = line[1:]
                if re.match(r'^\s*(scope|style|class|data-sort)', cell) and "|" in cell:
                    cell = cell.split("|", 1)[1]
                cells.append(cell)
        if cells:
            out.append(cells)
    return out


def heasarc_messier():
    url = ("https://heasarc.gsfc.nasa.gov/cgi-bin/W3Browse/w3query.pl?tablehead=name%3Dmessier"
           "&Action=Query&displaymode=BatchDisplay&ResultMax=0"
           "&Fields=name,alt_name,object_type,constell,vmag,dimension")
    out = {}
    for line in get(url).splitlines():
        f = [x.strip() for x in line.strip().strip("|").split("|")]
        if len(f) == 6 and re.match(r"M \d+$", f[0]):
            out[int(f[0][2:])] = {"alt": f[1], "type": f[2], "const": f[3],
                                  "mag": f[4], "dim": f[5]}
    return out


def messier_names():
    t = wiki("Messier object")
    t = t[t.index('{|class="wikitable plainrowheaders sortable'):]
    t = t[:t.index("\n|}")]
    names = {}
    for r in rows(t):
        m = re.search(r"\bM(\d+)\b", unlink(r[0]))
        if m and len(r) > 8:
            names[int(m.group(1))] = {"name": unlink(r[2]), "alt": unlink(r[1]),
                                      "type": unlink(r[4]).split("|")[-1],
                                      "const": unlink(r[6]), "mag": unlink(r[7]),
                                      "dim": unlink(r[8])}
    return names


def caldwell():
    t = wiki("Caldwell catalogue")
    t = t[t.index('{| class="wikitable sortable"'):]
    t = t[:t.index("\n|}")]
    out = {}
    for r in rows(t):
        m = re.search(r"C(\d+)", unlink(r[0]))
        if not m or len(r) < 8:
            continue
        out[int(m.group(1))] = {"alt": unlink(r[1]), "name": unlink(r[2]),
                                "type": unlink(r[4]), "const": unlink(r[6]),
                                "mag": unlink(r[7])}
    return out


def simbad(ids):
    """Position, kind and size for each identifier SIMBAD knows."""
    out = {}
    ids = sorted(set(ids))
    for i in range(0, len(ids), 80):
        chunk = ids[i:i + 80]
        inlist = ",".join("'%s'" % x.replace("'", "''") for x in chunk)
        q = ("SELECT i.id, b.ra, b.dec, b.otype, f.V, b.galdim_majaxis, b.galdim_minaxis, b.galdim_angle "
             "FROM ident AS i JOIN basic AS b ON b.oid=i.oidref LEFT JOIN allfluxes AS f ON f.oidref=b.oid "
             "WHERE i.id IN (%s)" % inlist)
        csv = get("https://simbad.cds.unistra.fr/simbad/sim-tap/sync",
                  {"REQUEST": "doQuery", "LANG": "ADQL", "FORMAT": "csv", "QUERY": q})
        for line in csv.splitlines()[1:]:
            f = next(__import__("csv").reader([line]))
            key = re.sub(r"\s+", " ", f[0]).strip()
            out[key] = f[1:]
    return out


def simbad_id(s, name=""):
    """How SIMBAD spells a catalog id: 'M 31', 'NGC 224', 'IC 342'.
    An object with no catalog number goes by its name ("NAME Coalsack")."""
    s = s.split("/")[0].split(",")[0].strip()
    if not re.search(r"\d", s):
        return "NAME " + re.sub(r" Nebula$", "", name)
    if s.startswith("Mel"):
        return "Cl Melotte " + re.sub(r"\D", "", s)
    if s.startswith("Sh"):
        return "SH 2-" + s.split("-")[-1]
    m = re.match(r"(M|NGC|IC|Mel|Cr|Sh2-|Sh 2-)\s*(\d+[A-Za-z]?)", s)
    if not m:
        return s
    cat = {"Sh2-": "Sh 2-", "Sh 2-": "Sh 2-"}.get(m.group(1), m.group(1))
    return ("%s%s" % (cat, m.group(2))) if cat.endswith("-") else ("%s %s" % (cat, m.group(2)))


# HEASARC / Wikipedia kind -> the chart's short kinds.
def kind_of(text):
    t = text.lower().strip()
    # HEASARC's codes, then SIMBAD's object types.
    t = {"di": "nebula", "ir": "galaxy", "e?": "galaxy", "pl": "planetary",
         "gb": "globular", "oc": "open", "opc": "open", "glc": "globular",
         "hii": "nebula", "rne": "nebula", "ism": "nebula", "dne": "dark",
         "**": "double star", "as*": "asterism", "*cl": "open", "cl*": "open",
         "c?*": "open"}.get(t, t)
    if t in ("g", "gig", "gip", "syg", "sy1", "sy2", "agn", "lin", "sbg", "emg",
             "bic", "gic", "ig", "h2g", "rg", "lsb", "g?"):
        return "Gx"
    if "dark" in t:
        return "DN"
    if "planetary" in t or t == "pn":
        return "PN"
    if "supernova" in t or t == "snr":
        return "SNR"
    if "globular" in t or t in ("gb", "gc"):
        return "GC"
    if ("cluster" in t and "neb" in t) or t in ("oc+neb", "cn"):
        return "CN"
    if "open" in t or "star cluster" in t or t in ("oc",):
        return "OC"
    if "nebula" in t or t in ("dn", "en", "rn", "hii", "neb"):
        return "Neb"
    if "galax" in t or t in ("s", "e", "i", "sb", "s0", "irr", "sa", "e/s0", "sab"):
        return "Gx"
    if "double" in t or "asterism" in t or "star" in t or t in ("ds", "ast", "sc"):
        return "Star"
    return "?"


def num(s):
    m = re.search(r"-?\d+(\.\d+)?", s or "")
    return float(m.group(0)) if m else None


def dims(s):
    """HEASARC dimension '11X10' or '80' (arcmin) -> (major, minor)."""
    parts = [num(x) for x in re.split(r"[xX×]", s or "") if num(x) is not None]
    if not parts:
        return None, None
    return parts[0], (parts[1] if len(parts) > 1 else parts[0])


# Where no source gives a size (mostly the big nebulae) or a magnitude,
# the usual published figures, rounded. Sizes in arcminutes. Kinds for
# the few that are neither cluster nor nebula: Ast is a star group
# (a double star, an asterism, a star cloud).
FILL = {
    "M24": {"kind": "Ast"}, "M40": {"kind": "Ast", "size": (0.8, 0.8)},
    "M73": {"kind": "Ast", "size": (2.8, 2.8), "mag": 8.9},
    "C4": {"size": (18, 18)}, "C20": {"size": (120, 100)}, "C27": {"size": (18, 13)},
    "C31": {"size": (30, 19)}, "C33": {"size": (60, 8)}, "C34": {"size": (70, 6)},
    "C41": {"size": (330, 330)}, "C46": {"size": (2, 1)}, "C49": {"size": (80, 60)},
    "C68": {"size": (1, 1), "mag": 9.7}, "C92": {"size": (120, 120)},
    "C99": {"size": (420, 300)}, "C100": {"size": (75, 75)},
}


# The 88 IAU constellations, full name -> the three-letter code the
# stick figures use.
IAU = dict(x.split(":") for x in """Andromeda:And Antlia:Ant Apus:Aps Aquarius:Aqr Aquila:Aql Ara:Ara Aries:Ari
Auriga:Aur Boötes:Boo Caelum:Cae Camelopardalis:Cam Cancer:Cnc Canes_Venatici:CVn Canis_Major:CMa
Canis_Minor:CMi Capricornus:Cap Carina:Car Cassiopeia:Cas Centaurus:Cen Cepheus:Cep Cetus:Cet
Chamaeleon:Cha Circinus:Cir Columba:Col Coma_Berenices:Com Corona_Australis:CrA Corona_Borealis:CrB
Corvus:Crv Crater:Crt Crux:Cru Cygnus:Cyg Delphinus:Del Dorado:Dor Draco:Dra Equuleus:Equ
Eridanus:Eri Fornax:For Gemini:Gem Grus:Gru Hercules:Her Horologium:Hor Hydra:Hya Hydrus:Hyi
Indus:Ind Lacerta:Lac Leo:Leo Leo_Minor:LMi Lepus:Lep Libra:Lib Lupus:Lup Lynx:Lyn Lyra:Lyr
Mensa:Men Microscopium:Mic Monoceros:Mon Musca:Mus Norma:Nor Octans:Oct Ophiuchus:Oph Orion:Ori
Pavo:Pav Pegasus:Peg Perseus:Per Phoenix:Phe Pictor:Pic Pisces:Psc Piscis_Austrinus:PsA
Puppis:Pup Pyxis:Pyx Reticulum:Ret Sagitta:Sge Sagittarius:Sgr Scorpius:Sco Sculptor:Scl
Scutum:Sct Serpens:Ser Sextans:Sex Taurus:Tau Telescopium:Tel Triangulum:Tri
Triangulum_Australe:TrA Tucana:Tuc Ursa_Major:UMa Ursa_Minor:UMi Vela:Vel Virgo:Vir Volans:Vol
Vulpecula:Vul""".split())
IAU = {k.replace("_", " "): v for k, v in IAU.items()}
CODES = {v.lower(): v for v in IAU.values()}


def iau(c):
    """Any spelling ('And', 'AND', 'Andromeda', 'Serpens Caput') -> 'And'."""
    c = c.strip()
    if c.lower() in CODES:
        return CODES[c.lower()]
    for name, code in IAU.items():
        if c.lower().startswith(name.lower()):
            return code
    return c


def main():
    hm = heasarc_messier()
    mn = messier_names()
    cw = caldwell()
    print("messier %d (names %d), caldwell %d" % (len(hm), len(mn), len(cw)), file=sys.stderr)
    ids = ["M %d" % n for n in range(1, 111)] + [simbad_id(c["alt"], c["name"]) for c in cw.values()]
    sb = simbad(ids)
    print("simbad answered %d of %d" % (len(sb), len(set(ids))), file=sys.stderr)

    lines = []
    missing = []

    def row(ident, alt, name, kind, key, mag, maj, mnr, const):
        fill = FILL.get(ident, {})
        kind = fill.get("kind", kind)
        if "size" in fill and not maj:
            maj, mnr = fill["size"]
        if mag is None:
            mag = fill.get("mag")
        s = sb.get(key)
        if not s:
            missing.append("%s (%s)" % (ident, key))
            return
        ra, dec, otype, vmag, smaj, smin, pa = s
        maj = maj or num(smaj)
        mnr = mnr or num(smin) or maj
        mag = mag if mag is not None else num(vmag)
        if kind == "?":
            kind = kind_of(otype)
        lines.append("\t".join([
            ident, alt, name, kind, "%.5f" % float(ra), "%.5f" % float(dec),
            "" if mag is None else "%.1f" % mag,
            "" if maj is None else "%.1f" % maj,
            "" if mnr is None else "%.1f" % mnr,
            "%d" % int(float(pa)) if pa not in ("", None) else "",
            iau(const)]))

    for n in range(1, 111):
        w = mn.get(n, {})
        h = hm.get(n) or w      # HEASARC first; Wikipedia where it has no row (M102)
        if not h:
            missing.append("M%d (no source row)" % n)
            continue
        maj, mnr = dims(h["dim"].replace("′", "").replace("″", ""))
        if n not in hm and "″" in w.get("dim", ""):
            maj, mnr = (maj or 0) / 60, (mnr or 0) / 60
        kind = kind_of(h["type"]) if h["type"].strip() else "?"
        const = h["const"]
        row("M%d" % n, h["alt"], w.get("name", ""), kind, "M %d" % n,
            num(h["mag"]), maj, mnr, const.title() if len(const) <= 3 else const)
    for n in sorted(cw):
        c = cw[n]
        row("C%d" % n, c["alt"], c["name"], kind_of(c["type"]), simbad_id(c["alt"], c["name"]),
            num(c["mag"]), None, None, c["const"])

    head = "# id\talt\tname\tkind\tra_deg\tdec_deg\tmag\tmajor_arcmin\tminor_arcmin\tpa_deg\tconstellation\n"
    open("data/dso.tsv", "w").write(head + "\n".join(lines) + "\n")
    print("wrote %d objects" % len(lines), file=sys.stderr)
    if missing:
        print("not found in SIMBAD: " + ", ".join(missing), file=sys.stderr)


if __name__ == "__main__":
    main()
