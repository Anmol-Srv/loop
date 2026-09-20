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
