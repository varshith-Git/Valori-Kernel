# Frontend rules

Scope: `ui/**` — the desktop-product Next.js app (App Router, React 19, Tailwind v4). Not the marketing
site/Cloud dashboard — that's the separate `valori-ui` repo.

**Source of truth:** [`ui/CLAUDE.md`](../../ui/CLAUDE.md) — the theming system, the surface-ladder token
table, and frontend coding principles are documented there in full (design-token values live in
`ui/src/app/globals.css`, not frozen into prose here — read the CSS, `ui/CLAUDE.md` is the map to it).
This file only summarizes what a Claude session must not violate, plus the root `CLAUDE.md`'s "UI — light
mode is mandatory" section, which still applies verbatim.

## Non-negotiable

1. **Never hardcode a structural color** — `oklch(...)`, `#hex`, `rgba(...)` — in a component. Every color
   resolves through a semantic token (`bg-card`, `bg-popover`, `text-muted-foreground`, …), defined once in
   `globals.css`'s `.dark`/`.light` blocks. A hardcoded color that happens to look right in one theme is a
   light/dark bug, not a style choice.
2. **Both themes, every change.** The app has a live theme toggle; real users use light mode. "Looks right
   in dark mode" is not done. See root `CLAUDE.md`'s checklist (7 items) before calling any UI change finished.
3. **Surface ladder, not more shades.** `ui/CLAUDE.md`'s "Surface ladder (60/30/10)" section documents the
   monotonic `background → sidebar → card → muted → popover → secondary → accent` hierarchy per theme.
   `muted`/`secondary`/`accent` must stay visually distinct — don't collapse them back to save a token, and
   don't add an 8th surface level without a real, distinct semantic role driving it.
4. **Elevation communicates Z-depth, not decoration.** `shadow-e-sm/md/lg` (defined in `globals.css`'s
   `@theme inline`) are semantic: `md` = dropdowns/popovers/menus, `lg` = dialogs/modals. Don't add
   `shadow-e-lg` to a `Card` because it "looks nice" — see `ui/CLAUDE.md`'s explicit guard comment at the
   token definitions.
5. **Fix the primitive, not the 52 consumers.** Shared primitives live in `ui/src/components/ui/` — a
   surface/token/interaction-state bug found in one consumer very often exists in every other consumer of
   the same primitive (`Card`, `Dialog`, `Button`, `Input`, `Tabs`, …). Check whether the bug is really in
   the primitive before patching call sites one at a time.
6. **Accessibility of a token change is a measured claim, not an assumption.** If you change a surface or
   text-color token, verify WCAG contrast from the *actual rendered pixels* (browser `canvas.fillStyle` +
   `getImageData`, or equivalent), not from the raw `oklch` lightness channel — `L³` is not a valid contrast
   proxy once any channel carries chroma. `ui/CLAUDE.md`'s "Accessibility" note under Theming has the method.

## Verification before calling frontend work done

```
cd ui && npx tsc --noEmit -p .      # or: node_modules/.bin/tsc --noEmit
cd ui && npm run build              # production build — also the fullest typecheck
```
Then verify visually via the Browser tool in both themes for anything touching layout/color — a passing
typecheck proves nothing about the actual rendered hierarchy.
