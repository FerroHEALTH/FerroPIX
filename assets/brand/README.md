<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# FerroPIX brand

FerroPIX follows the FerroHEALTH family system and takes its own mark and its
own hue, as every product in the family does. The file set, the naming and the
variants are shared with the family; the mark and the palette are FerroPIX's
own. Everything here is under the Business Source License 1.1 with the rest of
the repository.

## The mark

An index card with the patient on it and their identifiers beside them. The product is a Master Patient Index, and identifying data lives on the index, outside the record. The tab is the full hue; the person and the identifiers take the light value.

## Palette, "Plum & Iron"

| Token | Hex | Use |
|---|---|---|
| plum | `#6B21A8` | primary mark and accents; text on light |
| plum-light | `#C084FC` | the same voice on a dark ground; highlights in the mark |
| ink | `#0F172A` | text on light |
| mist | `#F1F5F9` | text on dark |
| tile | `#0B1020` | dark tile background |
| surface | `#F8FAFC` | light surface background |

The hue was chosen by measurement against the hues the family already owns:
worst-case CIEDE2000 distance over both grounds and normal, protanope and
deuteranope vision. The numbers and the bar are recorded in the family
repository's `assets/brand/README.md`, and the family site copies the two hue
values from `tokens.css` here verbatim.

## Files

| File | What it is |
|---|---|
| `ferropix-icon.svg` | primary icon, full colour, transparent background, 64-unit viewBox at a 512 intrinsic size |
| `ferropix-icon-dark.svg` | the icon for a dark ground: the card and the tab in plum-light (`#C084FC`), the person and the identifiers in `#F3E8FF` |
| `favicon.svg` | the tightened mark on a plum tile, for browser tabs; intrinsic size 64 |
| `favicon-32.png`, `favicon-16.png`, `favicon.ico` | raster favicons |
| `ferropix-lockup-auto.svg` | the mark and the wordmark side by side, switching between the light and dark tones on `prefers-color-scheme`, for the README and any page that serves one file to both themes |
| `tokens.css` | the palette as CSS custom properties |

The social card follows when the product needs it.

## Where the palette applies

The landing page reads these values through its own `--fp-*` tokens in
`website/landing/style.css`, and the book recolours mdBook's `light` and
`navy` themes with them in `website/book/theme/ferropix.css`, so the site,
the book and the mark agree. The book shows `ferropix-icon.svg` beside its
title in the light theme and `ferropix-icon-dark.svg` in the dark one.

The book has no copy of the favicon: `scripts/site/assemble.sh` builds it
from a staged copy that adds `favicon.svg` and `favicon-32.png` from here to
the theme directory mdBook reads, so the repository keeps one copy of the
artwork, and copies this directory to `assets/brand/` of the assembled site.

## Regenerating the rasters

The PNG and ICO files derive from `favicon.svg`. Run these from the
repository root after any change to it:

```bash
rsvg-convert -w 32 -h 32 assets/brand/favicon.svg -o assets/brand/favicon-32.png
rsvg-convert -w 16 -h 16 assets/brand/favicon.svg -o assets/brand/favicon-16.png
magick assets/brand/favicon-32.png assets/brand/favicon-16.png assets/brand/favicon.ico
```
