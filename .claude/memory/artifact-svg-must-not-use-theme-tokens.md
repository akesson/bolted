---
name: artifact-svg-must-not-use-theme-tokens
description: In claude.ai artifacts, SVG fills must be currentColor/alpha — the viewer's dark transform inverts HTML colors but leaves SVG fills alone
metadata:
  type: feedback
---

Inline SVG diagrams in a published artifact rendered unreadable (light-on-light) in the
owner's dark viewer, even though local light/dark `data-theme` tests passed.

**Why:** the artifact viewer's dark-mode transform inverts HTML text/background colors but
leaves SVG shape fills untouched. Theme-token fills (`fill: var(--panel)`) stay light while
`currentColor`-derived text gets inverted to light → light text on light boxes. Testing only
the `prefers-color-scheme` / `data-theme` paths misses this failure mode entirely.

**How to apply:** make artifact SVGs theme-independent by construction — text and lines use
`fill/stroke: currentColor` (+ `opacity` for muted; `fill-opacity` on `tspan`, plain `opacity`
is invalid there), box fills use translucent greys (`rgba(127,140,148,.06–.13)`), and only
mid-tone hues that read on both grounds get literal colors. Never `var(--panel/--ink/--muted)`
inside SVG. Also verify figures by screenshotting each one with Playwright at device scale —
full-page thumbnails hide label overflow and clipping.
