# Valori × Jev Page — Design

**Repo:** `valori-ui` (spec lives in `Valori-Kernel` per this project's established convention).

## Goal

A new standalone marketing route, `/jev`, that explains a new architecture —
Valori reconstructs state, Jev turns state into typed probabilistic
decisions, application code stays the authority — using Valori's own
existing visual language, not a new one. Plus a teaser section on the home
page in the second product/storytelling slot, a nav entry, and sitemap/SEO
wiring.

## Audit findings

| Question | Answer |
|---|---|
| Where does homepage content live? | `content/marketing/home.mdx` — the whole homepage is MDX composed from shortcodes (`MarketingHero`, `TrustBar`, `ProductShowcase`, `MarketingSection`, `ArchitectureDiagram`, `FeatureGrid`, `UseCaseGrid`, `FinalCta`, etc.), rendered via `marketingMdxComponents` (`components/marketing/marketing-mdx-components.tsx`) through `<MDXRemote>` in `app/page.tsx`. |
| Precedent for a dedicated, heavily-custom marketing page? | `app/solutions/rag/page.tsx` — a plain `.tsx` route with a direct `export const metadata`, delegating to one big custom component (`RagDemoPage`). Not MDX. `/jev` needs far more custom diagram/animation work than MDX shortcode composition supports cleanly, so it follows **this** precedent, not the home.mdx pattern — while still importing and reusing the shared marketing components (`Section`, `Container`, `Button`, `FeatureGrid`, icon registry) rather than inventing new ones. |
| The vertical-rhythm primitive | `components/layout/section.tsx`'s `<Section>` — `py-16 lg:py-20`, `border-t` divider (suppressed on `:first-child`), `width` ∈ `narrow (max-w-2xl) / medium (max-w-3xl) / wide (max-w-5xl) / full`, optional `center`. `MarketingSection` (`components/marketing/marketing-section.tsx`) is a thin wrapper adding eyebrow/title/description above `<Section>`'s children — I'll reuse `<Section>` directly for `/jev`'s custom sections (since most need custom headings that don't fit `MarketingSection`'s fixed eyebrow+h2+description shape) and `MarketingSection` where the shape does fit (e.g. the product-value grid). |
| Page-width primitive | `components/layout/container.tsx`'s `<Container>` — `width` ∈ `prose/standard/wide/docsShell/apiShell/fluid`. Home uses `width="fluid"` (Tailwind's own stepped container, caps at 1536px). `/jev` uses the same. |
| Hero pattern | `components/marketing/marketing-hero.tsx` — eyebrow pill (`rounded-full border border-primary/40 bg-muted/60 ... font-mono text-xs uppercase tracking-wider text-primary-on-tint`), `h1` at `text-3xl sm:text-4xl md:text-5xl xl:text-[46px]`, two-column grid on `xl` (copy left, visual right), two CTAs (`Button` default variant + `Button variant="outline"`). This shortcode is MDX-specific (imports `AuthCTA` for the primary CTA, which is homepage-specific behavior — logged-in vs anon label). `/jev`'s hero needs different, fixed CTA labels (`Start building` / `See the architecture`, not the dashboard-aware `AuthCTA` swap) — so `/jev` gets its own hero markup built from the same classes/primitives (`Button`, the same eyebrow pill, the same heading scale) rather than reusing `MarketingHero` directly. |
| The diagram idiom already in use | `components/marketing/architecture-diagram.tsx` — `rounded-lg border border-border bg-card` mono-font boxes, `h-6 w-px bg-border` thin vertical connectors, and a "highlighted" box variant (`border-primary/40 bg-primary/5` → `border-primary/60 bg-primary/10 font-semibold text-primary-on-tint` for the most-important box). This exact idiom (box/connector/highlight) is the visual language every diagram on `/jev` should extend — not SVG art, not illustration. |
| Feature-grid pattern | `components/marketing/feature-grid.tsx` — icon in a `bg-primary/10 text-primary` rounded square, `h3` + one paragraph, 2/3/4-column responsive grid. Reused as-is for the "Product Value" section. |
| Closing CTA pattern | `components/marketing/final-cta.tsx` — centered, `max-w-xl`, `h2` + paragraph + two buttons. **Not reusable as-is**: it hardcodes "Get Started"/"Documentation" labels and has no eyebrow slot, but `/jev`'s final CTA needs a custom eyebrow, a two-line heading, and different button labels/hrefs (`Start building` / `Read the docs`, not the homepage's `AuthCTA`-driven pair). Built as page-local markup reusing the same classes, not by widening the shared component's prop surface for one caller. |
| Fonts | Inter (sans, variable, local via `next/font/local`) + JetBrains Mono (mono) — `--valori-font-sans` / `--valori-font-mono` in `globals.css`, aliased to Tailwind's `font-sans`/`font-mono`. Diagram boxes, eyebrows, and code already use `font-mono`; body copy uses the sans default. No new fonts. |
| Theming | Confirmed via `CLAUDE.md`: light mode support is a non-negotiable project rule, not optional — every token used must be semantic (`bg-card`, `text-muted-foreground`, `border-border`, `text-primary-on-tint`, etc.), never a raw hex/oklch literal. The existing components audited above already follow this throughout; `/jev`'s new markup does the same. No dark-only shortcuts. |
| Nav | `lib/nav/marketing-nav.ts` — typed `NavLeaf`/`NavColumn`/`MegaMenu`. The `PRODUCTS_MENU.columns[0].items` array (Valori Cloud, Valori Vector Database, Valori Desktop, Valori Enterprise) is the natural, minimal home for one new `NavLeaf` (`title: 'Valori × Jev', href: '/jev'`) — adds one line, doesn't touch menu structure or crowd the top-level bar, matching "do not clutter the top-level nav." |
| Sitemap | `app/sitemap.ts` — flat `staticPaths` array of public route strings. Add `/jev` unconditionally (no feature flag needed, unlike `/solutions/rag`'s `isDemoRagEnabled()` gate). |
| Footer | `components/marketing/site-footer.tsx`, already rendered globally by the root layout (confirmed: `solutions/rag/page.tsx` renders no footer of its own, relying on the shared layout) — `/jev` needs zero footer code, it inherits the same footer automatically by living under the same route group. |
| `ProductShowcase` (home.mdx's post-hero section) | The true "first primary product section" right after the hero — it's still part of the homepage's opening beat (search/connect/verify screenshots), not a distinct storytelling section. The Valori × Jev teaser therefore slots in as the section **immediately after `<ProductShowcase>` and before** the existing `<MarketingSection eyebrow="What is Valori?" ...>` — this is "the second major product/storytelling slot" the brief asks for. |

## Page architecture: `/jev`

`app/jev/page.tsx` — server component, `export const metadata = {...}` (title/description/canonical/OG/Twitter, per the SEO section below), renders `<JevPage />`.

`components/jev/JevPage.tsx` — the page body, `<div className="min-h-screen"><Container width="fluid">...</Container></div>`, composing one section component per named section from the brief, each using `<Section>` for rhythm and the box/connector diagram idiom for visuals. One file per section (see File Structure) — each is independently a few hundred lines at most for the more diagram-heavy ones, keeping files focused per this project's own "smaller, well-bounded units" preference.

Diagrams are built as small presentational components sharing one local primitive module (`components/jev/diagram-primitives.tsx`: `DiagramBox`, `DiagramConnector`, `DiagramArrow`) extending `architecture-diagram.tsx`'s exact class idiom, so every diagram on the page — hero collapse, prompt-stack, vector/graph/timeline/verify stages, the big system diagram, the traditional-vs-Valori×Jev split, the compilation pipeline, the one-state-many-decisions branch, the evidence graph — reads as one consistent visual system, not eight different ad hoc treatments.

### Animation

CSS-only (Tailwind's `animate-*` utilities plus a handful of small local `@keyframes` in a scoped `<style jsx>` or a `jev.css` module if needed) — no new animation library. Every animated element is wrapped so `@media (prefers-reduced-motion: reduce)` disables the animation and shows the end state directly (a `motion-reduce:` Tailwind variant on each animated class, or a `motion-reduce:animate-none` override) — per the brief's explicit requirement and this being the kind of thing easy to silently miss.

### Responsive strategy

Every diagram is authored mobile-first as a **vertical** flow (flex-col, connectors as horizontal-then-rotated-to-vertical thin lines) and only becomes a horizontal/branching layout at `lg:`/`xl:` breakpoints where there's room — matching the brief's explicit "convert horizontal flows into vertical flows on mobile, do not horizontally scroll." Code blocks use `overflow-x-auto` inside a fixed-width container (existing pattern already used by the docs/API code components) rather than ever forcing page-level horizontal scroll.

## Section-by-section content mapping

Each section below is implemented as its own component; copy is taken **verbatim** from the brief (hero eyebrow/title/subheading/buttons, problem-section heading/copy, stage copy, Jev's example state/questions/output, the whole-system diagram's exact box contents, the real-use-case's two-column scenario and the exact `if (...)` code block, the evidence-graph relationships and question list, the LLM-vs-Valori×Jev split's two lists, the compilation-pipeline stages, the one-state-many-decisions example, the product-value grid's 9 items, and the final CTA's exact copy) — the plan's tasks carry this copy exactly, not paraphrased, so there's no drift between this doc and what ships.

1. Hero — custom markup (not `MarketingHero`, see audit), large collapse diagram below the copy.
2. The Problem — prompt-stack diagram + the two callout lines as visually prominent standalone statements (large type, not body paragraphs).
3. Valori Builds The State — 4 stages (vectors / graph / time / verification), each with its own small diagram, "Deterministic memory. Probabilistic intelligence." as a prominent standalone line between stages 3 and 4.
4. Then Jev Decides — deliberately visually simpler than what came before (smaller diagram footprint, more whitespace) per the brief's explicit "visually simplify the page at this moment."
5. The Whole System — the big box diagram, verbatim structure from the brief, with the "Remember → retrieve → connect → reconstruct → decide → act → record → verify" line underneath.
6. Real Use Case — two-column (stacks to one column on mobile) traditional-vs-Valori×Jev comparison, decision output, and the real TypeScript policy snippet.
7. Why Did The Agent Do That — evidence graph + the 9-item question list.
8. Use The Right Intelligence — split layout (LLM column / Valori×Jev column), "The LLM becomes a specialist instead of the operating system" as the connecting statement. No anti-LLM framing anywhere, matching the brief's explicit prohibition.
9. Context Should Be Compiled — the compression pipeline (10,000,000 → 23 → 7 → structured state → 4 decisions), progressively shrinking visual treatment.
10. One State, Many Decisions — the branching diagram (state → 3 parallel typed decisions with probabilities).
11. Product Value — `<FeatureGrid columns={3}>` with the 9 items from the brief, reusing the shared component as-is (this is the one section whose shape genuinely matches `FeatureGrid`'s contract).
12. Final CTA — page-local markup (see audit — `FinalCta` doesn't fit), eyebrow + two-line heading + supporting copy + the three-line "Valori provides / Jev provides / Your code provides" + two buttons (`Start building` → wherever the homepage's primary CTA points, e.g. `/dashboard` or signup; `Read the docs` → `/docs`).

Footer: none — inherited from the root layout automatically.

## Landing page teaser

New MDX shortcode `JevTeaser` (`components/marketing/jev-teaser.tsx`, registered in `marketing-mdx-components.tsx`), inserted into `content/marketing/home.mdx` right after the `<ProductShowcase .../>` block and before the existing `<MarketingSection eyebrow="What is Valori?" ...>` block. Contents: eyebrow "VALORI × JEV", heading, body, and a compact vertical box/connector diagram (Memory → Valori → State → Jev → Decision → Code) using the same `DiagramBox`/`DiagramConnector` primitives as `/jev` itself (imported from `components/jev/diagram-primitives.tsx` — shared, not duplicated), plus one CTA (`Explore Valori × Jev →`) linking to `/jev`. Kept short per the brief's explicit "should not be excessively tall" — a single `<Section>` with `width="medium"`, not a multi-stage page-length treatment.

## SEO

Confirmed against `app/layout.tsx`'s root metadata (the actual shape this codebase uses, not guessed): the root layout sets `openGraph`/`twitter` defaults including `siteName: 'Valori'` and `images: ['/assets/logo.png']`, with an explicit comment warning that **Next.js does not deep-merge** — a child page that declares its own `openGraph` object fully replaces the root's, silently losing `siteName`/`images` unless the page repeats them. No existing page overrides `openGraph` today (confirmed by grep), so `/jev` is the first — it must repeat both fields, exactly as the root comment instructs:

```ts
const TITLE = 'Valori × Jev | Memory to Decision Without an LLM in the Middle'
const DESCRIPTION = "Valori reconstructs relevant, connected, point-in-time AI memory. Jev turns that state into typed decisions with probabilities. Build auditable AI systems without routing every decision through an LLM."

export const metadata: Metadata = {
  title: TITLE,
  description: DESCRIPTION,
  alternates: { canonical: '/jev' },
  openGraph: {
    type: 'website',
    url: '/jev',
    siteName: 'Valori',
    title: TITLE,
    description: DESCRIPTION,
    images: ['/assets/logo.png'],
  },
  twitter: {
    card: 'summary_large_image',
    title: TITLE,
    description: DESCRIPTION,
    images: ['/assets/logo.png'],
  },
}
```

## Out of scope / explicit non-goals (per the brief)

- No new design tokens, no new color palette, no new typography scale.
- No claim that Jev itself is deterministic — every mention pairs "deterministic/verifiable state" with "probabilistic decision," never collapses the two.
- No "LLMs are obsolete" framing anywhere.
- No fabricated hashes, benchmark numbers, or two matching state hashes presented as a real cross-environment replay demo — the state-hash/proof section illustrates the existing verification *concept* (state roots, BLAKE3 receipts — real, already-documented Valori capabilities per `content/marketing/home.mdx`'s own "Why deterministic?" section) without fabricating a live two-machine comparison that doesn't exist in the product today.
- Top-level nav gets exactly one new line (Products column), nothing else restructured.

## Testing

This codebase's Vitest coverage (`vitest.config.ts`'s explicit include-list, confirmed during today's earlier login-method work) doesn't cover component rendering — no jsdom/RTL setup exists project-wide. Consistent with that established precedent (and `marketing/use-case-grid.test.ts`/`footer-policy.test.ts`'s actual scope, which test pure logic, not rendering), the tests added here are:

- `sitemap.test.ts`-equivalent check (or extend if one exists) confirming `/jev` is present in `sitemap()`'s output.
- A pure-logic test for any data/copy transformation function introduced (if the implementation ends up with one, e.g. a step-index calculator for the compression animation) — added only if such a function actually exists, not manufactured to have something to test.
- Manual verification: `npx tsc --noEmit`, `npm run lint`, `npm run build`, and a manual read-through of the rendered route at each of the 9 specified breakpoints is the realistic verification bar for a visual page in a codebase with no component-rendering test infrastructure — same bar `/solutions/rag` (this page's own structural precedent) was held to.
