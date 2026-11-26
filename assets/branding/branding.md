# Eario visual identity

## Idea

The mark shows three data points rising toward a four-point star: scattered
measurements lead the eye to the light that makes sense of them. The star is
a quiet nod to the light the project is named after; nothing else in the
identity refers to it. In the wordmark, the star replaces the dot of the *i*.

## Files

| File | Use |
| --- | --- |
| `icon.svg` | Square mark, for avatars and application icons. Works on any background. |
| `favicon.svg` | Same drawing as `icon.svg`, served by the frontend. A copy lives in `frontend/public/`. |
| `logo.svg` | Mark and wordmark, for light backgrounds. Default choice. |
| `logo-light.svg` | Same as `logo.svg`. |
| `logo-dark.svg` | Mark and wordmark, for dark backgrounds. |

All files are plain vector shapes. The wordmark is drawn with strokes, not
set in a typeface, so no font is needed to render it.

## Colours

| Name | Value | Role |
| --- | --- | --- |
| Deep blue | `#12305a` | Background of the mark, wordmark on light backgrounds |
| Teal | `#0f8b8d` | First data point, accent of the user interface |
| Sea green | `#35b3b0` | Second data point |
| Pale cyan | `#9fe3e0` | Third data point |
| Starlight | `#f5c66a` | The star |
| Amber | `#d99a1c` | The star of the wordmark on light backgrounds, where starlight is too pale |
| Mist | `#e6edf5` | Wordmark on dark backgrounds |

The user interface takes its accent from the teal. Its full palette is
defined in `frontend/src/app.css`.

## Small sizes

`icon.svg` is drawn on a 64-unit grid. The smallest data point is 6.4 units
wide and the star 28, so at 16 pixels the star stays 7 pixels wide and each
point at least 1.6. Do not add detail to the mark; it would not survive that
size.

## Monochrome version

For single-colour printing or masks, derive a monochrome mark from `icon.svg`:

1. Remove the `rect` background.
2. Set the `fill` of the three circles and of the star to `currentColor`.

The shapes do not overlap, so the result reads the same in one colour.

## Clear space and misuse

Keep clear space of at least a quarter of the mark's height around the logo.
Do not recolour the star, rotate the mark, or set the wordmark in a typeface.
