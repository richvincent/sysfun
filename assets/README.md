# sysfun brand assets

The mark is a terminal panel whose gauge draws a smile: friendly first, instrument underneath.

| file | use |
|------|-----|
| `mark.svg` | colour icon, 128 viewBox, ivory tile. App icons, favicons, social avatars. |
| `mark-mono.svg` | single-ink icon drawn with `currentColor`, no tile. Man pages, badges, terminal tab icons, anything one-colour. |
| `logo.svg` | stacked logo (icon over wordmark) on an ivory tile. Square placements. |
| `logo-horizontal-light.svg` | icon, wordmark and tagline for light backgrounds, transparent. |
| `logo-horizontal-dark.svg` | the same lockup for dark backgrounds. |
| `png/` | raster exports: the mark at 16–512 px, the stacked logo at 1024 px, banners at 1560 × 640. |

Palette: ivory `#FAF7EF`, ink `#172D35`, slate `#486169`, teal `#5EC4C4`, amber `#E6BE5A`.
Wordmark and tagline are JetBrains Mono (Bold and Medium), converted to outlines so every file renders identically without fonts installed.

Regenerate the PNGs with `rsvg-convert -w <size> -h <size> mark.svg -o png/mark-<size>.png`.
