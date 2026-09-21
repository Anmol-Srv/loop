# Design mockups

HTML mockups of each screen, and the CSS that drives them.

They exist because the agent building this app cannot screenshot a running
macOS window — no screen-recording grant — so a purely visual bug reaches the
user unverified. Rendering the same design as HTML in headless Chromium gives
a real feedback loop: build, screenshot, look, iterate, and only then write the
egui version against a target that has been seen.

`tokens.css` mirrors `src/desktop/design/tokens.rs` value for value. **If one
changes, change the other** — a mockup that drifts from the tokens is worse
than no mockup, because it looks authoritative.

Regenerate a screenshot:

    browse goto file:///path/to/docs/design-mocks/home.html
    browse screenshot home.png

## v2 — the dashboard rebuild

`home2.html` + `tokens2.css`. The first pass built a spreadsheet: flat rows in
flat containers, one type size, no focal point. The references it is now built
against share five things the first pass had none of:

1. **A task is a card with internal hierarchy**, not a row. Meta chips on top,
   the title as the loudest thing, context and people underneath.
2. **A stat row leads the page.** Big numerals, a delta chip, one sparkline.
3. **Two columns.** A right rail was tried and dropped: it duplicated the
   sidebar's project list and drew a capacity chart from data the API does not
   return. Team capacity survived the cut — as a band at the foot of the work
   column, built from open task counts, which is real.
4. **The sidebar is grouped** with small muted group labels, a search
   affordance, live project and agent entries, and the signed-in user pinned
   at the foot.
5. **Tinted pills carry state**, one hue per meaning, instead of grey text.

Two bugs the render caught that reading could not:

- A glyph-in-pill (`◑`) fell back to a font Nunito does not cover, so the
  measured width was wrong and the label overflowed its own background.
- `class="tag prog"` collided with `.prog`, the progress-bar rule in the rail:
  the status pill inherited `height:5px; overflow:hidden` and was squashed
  into a clipped sliver. Measuring the box against the text is what found it;
  three visual passes had not.

### Three bugs the render caught, none of which reading would have

- A glyph in a pill (`◑`) fell back to a font Nunito does not cover, so the
  measured width was wrong and the label overflowed its own background.
- `class="tag prog"` collided with `.prog`, the progress-bar rule: every status
  pill inherited `height:5px; overflow:hidden` and was squashed to a clipped
  sliver. Two attempted fixes (padding, flex) changed nothing, because the
  problem was a name collision. Measuring the box against the required text
  width is what found it.
- `file://` loads were served from cache, so a screenshot showed the previous
  version of the page while the DOM query showed the new one. Append a query
  string to force a reload before believing a screenshot.
