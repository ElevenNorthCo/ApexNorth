# apexnorth.app

The ApexNorth website: one static page (`index.html`), no build step.
`.github/workflows/pages.yml` publishes it to GitHub Pages on every push to
`main` that touches `site/`, the screenshots or the install scripts. It copies
in `docs/screenshots/*.jpg` (as `screenshots/`), the 256 px app icon and
`install.sh` / `install.ps1`.

Downloads link to `releases/latest/download/<fixed installer name>`, so the
page never needs editing for a new version; the version label is read from
the GitHub API in the browser.

Preview locally: `cd site && python3 -m http.server` (screenshots and icon
are only there once the workflow copies them).
