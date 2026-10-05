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
| `tokens.css` | the palette as CSS custom properties |

The lockups, the favicon set and the social card follow when the product has a
site to carry them.
