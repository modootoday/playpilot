# playpilot brand assets

Sources are the SVG files; every PNG is rendered from them at an exact pixel size.

| File | Use |
| --- | --- |
| `mark.svg` | Master mark (rounded tile). Also `favicon.svg`. |
| `mark-square.svg` | Full-bleed tile for `apple-touch-icon.png` (iOS rounds it). |
| `mark-maskable.svg` | Full-bleed tile, glyph inside the 80% safe zone, for `icon-maskable-512.png`. |
| `favicon-32.png`, `apple-touch-icon.png` (180), `icon-192.png`, `icon-512.png`, `icon-maskable-512.png` | Web favicon and web app manifest set, same names as jekyll-theme-modootoday. |
| `../plugin/.claude-plugin/icon.png` (1024) | Claude Code plugin listing icon (PNG required). |

Tokens: tile `#1F4482` → `#0A1630` (diagonal), wing `#FFFFFF` → `#E4ECFF`, lower wing `#A9C2F2`, fold `#7C9CDA`, pilot light `#F26B3A`.

## With jekyll-theme-modootoday

Copy the PNG and SVG files to `assets/playpilot/` in the docs site, leave
`modootoday.favicon` unset, and add the links in `_includes/head-custom.html`:

```html
<link rel="icon" href="{{ '/assets/playpilot/favicon.svg' | relative_url }}" type="image/svg+xml">
<link rel="icon" href="{{ '/assets/playpilot/favicon-32.png' | relative_url }}" type="image/png" sizes="32x32">
<link rel="apple-touch-icon" href="{{ '/assets/playpilot/apple-touch-icon.png' | relative_url }}">
```

The web app manifest icons are `icon-192.png`, `icon-512.png` and `icon-maskable-512.png` (`purpose: maskable`).
