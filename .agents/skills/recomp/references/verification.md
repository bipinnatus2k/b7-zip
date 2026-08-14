# URL Verification

Goal: decide whether a fetched URL is a valid input for replication — a
*component library documentation page for a single UI component*.

## Procedure

1. Fetch the page (WebFetch).
2. If the fetch fails (login wall, heavy client-side rendering), say so and
   propose alternatives: the library's GitHub source, npm package README, or a
   different docs URL. Do not proceed.
3. Apply the signal checklist below and output the verdict.

## Signals of a component library docs page

- Component name in title / URL (e.g. `/components/dialog`)
- Props / API Reference table (prop name, type, default, description)
- Usage / Example code blocks
- Installation snippet (npm / pnpm / bun)
- UI-component semantics: the documented object has DOM structure, visual
  states, interaction props (`variant`, `size`, `disabled`, `open`,
  `onOpenChange`), and/or Accessibility / Keyboard Interaction sections

Non-web libraries (Flutter / SwiftUI / Jetpack Compose / Qt packages,
mini-program component libraries such as Vant Weapp / TDesign) use the
same structural signals — component name, API reference table, usage
examples, installation snippet. The ARIA / Keyboard Interaction signal
applies to web stacks only; for non-web stacks, look for the platform's
accessibility documentation instead.

## Exclusion table — refuse with a reason

| Input | Verdict | Suggested path |
|---|---|---|
| Blog / tutorial article | refuse | send a real docs page |
| Framework official docs (component section of Vue/React docs) | refuse | — |
| Utility library (date-fns, lodash) | refuse | API docs, but no UI components |
| Design file / mockup | refuse | different workflow (visual replication) |
| Library homepage / component index | partial | list detected components, confirm which one |
| Unreachable (JS-rendered, auth) | partial | npm / GitHub source as input |

## Verdict formats (one line in conversation)

- `PASS`: "Validated: <library> <component> docs page"
- `PARTIAL`: library but not a single-component page → confirm target from index
- `REFUSE`: not a component library + reason
