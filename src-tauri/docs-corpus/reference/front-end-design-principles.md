# Front-End Design Principles

Reference knowledge for building interfaces, oriented to citrate-core's design bar:
translate a designer's prototype faithfully, and never present fabricated data as real.

## Honesty over decoration

The most important UI rule in a node app is that every surface states what is real. A
value read from the chain shows the live figure or an honest error, never a placeholder
dressed as data. A feature that is not wired says so plainly. An empty state is a real
state with its own copy ("nothing yet, here is how it fills"), not a spinner that never
resolves. Trust is the product; a single fabricated number costs it.

## Hierarchy and layout

Establish a visual hierarchy: size, weight, and spacing tell the eye what matters first.
Group related controls; separate unrelated ones with whitespace, not just lines. A
consistent spacing scale and a small type scale make a dense app feel calm. Align to a
grid so the layout reads as intentional.

## State, feedback, and latency

Every action needs feedback within about 100 ms, even if only a pending state. Long
work shows progress, and the UI stays responsive while it runs — never freeze the
window on a blocking call. Optimistic updates feel fast but must reconcile honestly when
the real result arrives, rolling back visibly if it failed. Auto-scroll a transcript to
the newest content after it commits, not before, so the view tracks reality.

## Forms and input

Validate at the moment of input and explain a rejection in place, next to the field.
Preserve what the user typed across an error. Capture a value synchronously inside an
event handler rather than reading a possibly-nulled event target later inside an async
update. Disable a submit only when you can say why, and say why in a tooltip.

## Accessibility and contrast

Meet contrast minimums for text; do not encode meaning in color alone — pair it with a
label or shape. Support keyboard navigation and visible focus. Respect the viewer's
light/dark preference and reduced-motion settings. Legible defaults beat clever ones.

## Theming and reuse

Drive color, spacing, and radius from tokens (CSS variables) so a theme is one source of
truth and both light and dark are first-class. Build presentational components that take
data as props and hold no business logic, so they are testable in isolation and reused
across surfaces. Keep the data-fetching seam separate from the rendering seam.
