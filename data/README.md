# Data

Two plain-text tables, compiled into the library with `include_str!`.

## `stars.csv`

`ra,dec,mag,bv,spectral,hd,hip,dist_pc,name` — 9,096 stars, brightest
first. Right ascension and declination are J2000 degrees, `mag` is
visual magnitude, `bv` the B−V colour index, `spectral` the type as the
catalogue spells it, `hd` / `hip` the cross-identifications, `dist_pc`
the distance in parsecs (empty where the parallax is negative or worse
than 20% uncertain, which is 19% of the catalogue), `name` the IAU
proper name where the star has one (333 of them).

Positions, magnitudes, colours and spectral types come from the **Yale
Bright Star Catalogue, 5th Revised Edition** (Hoffleit & Warren 1991),
distributed by the Astronomical Data Center. Public domain.
<http://tdc-www.harvard.edu/catalogs/bsc5.html>

Distances are derived from the parallaxes in **The Hipparcos and Tycho
Catalogues** (ESA 1997, ESA SP-1200), retrieved from VizieR (I/239),
matched to the Yale catalogue on HD number. 8,995 of the 9,096 match.
<https://cdsarc.cds.unistra.fr/viz-bin/cat/I/239>

Proper names come from the **IAU Catalog of Star Names** (IAU Working
Group on Star Names), matched on HR number.
<https://www.pas.rochester.edu/~emamajek/WGSN/IAU-CSN.txt>

## `constellations.csv`

`ABR:ra,dec ra,dec …` — one polyline per line, 150 of them, in J2000
degrees. Several strokes per constellation, since the stick figures
branch.

From **d3-celestial** by Olaf Frohn, BSD 3-clause:
<https://github.com/ofrohn/d3-celestial>

    Copyright (c) 2015-2020, Olaf Frohn
    All rights reserved.
    Redistribution and use in source and binary forms, with or without
    modification, are permitted provided that the conditions of the
    BSD 3-clause licence are met.

The library itself is public domain (Unlicense); this one file carries
Olaf Frohn's notice with it.

## `dso.tsv`

`id, alt, name, kind, ra_deg, dec_deg, mag, major_arcmin, minor_arcmin,
pa_deg, constellation`, tab-separated: the 110 Messier objects and the
109 Caldwell objects. `tools/build-dso.py` writes it; the library never
goes online.

- **Kind, magnitude and size of the Messier objects**: NASA HEASARC's
  Messier table. Public domain.
  <https://heasarc.gsfc.nasa.gov/W3Browse/all/messier.html>
- **Positions, and sizes and tilts where HEASARC has none**: SIMBAD,
  operated at CDS, Strasbourg, France. Free to use; this data has made
  use of the SIMBAD database.
  <https://simbad.cds.unistra.fr/>
- **Which NGC or IC object each Caldwell number is, common names, and
  the Caldwell magnitudes**: Wikipedia's Messier and Caldwell tables.
  Only these facts are taken, none of the text.
- **Sizes of 15 large nebulae and star groups** that no source above
  gives: the usual published figures, rounded, listed in the script.

`kind` is one of Gx (galaxy), OC (open cluster), GC (globular), PN
(planetary nebula), Neb (nebula), CN (cluster with nebula), SNR
(supernova remnant), DN (dark nebula) and Ast (a double star, an
asterism or a star cloud).
