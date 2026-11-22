# Yonder website

The marketing site and docs for [Yonder](https://github.com/RMoura98/yonder). Plain HTML, CSS and vanilla JavaScript: no build step, no dependencies.

```
website/
├── index.html      # landing page
├── docs.html       # single-page documentation
├── 404.html        # not-found page (GitHub Pages serves it automatically)
├── .nojekyll       # tells Pages to skip Jekyll processing
└── assets/
    ├── logo.svg    # app icon, also the favicon
    ├── logo-mono.svg # single-colour glyph
    ├── styles.css  # all styles (light + dark via prefers-color-scheme)
    └── main.js     # OS detection, copy buttons, scroll reveal, docs scrollspy
```

## Preview locally

Any static file server works, for example:

```sh
cd website
npx serve .            # or: python -m http.server 8000
```

## Deploy to GitHub Pages

Deployment is handled by [`.github/workflows/pages.yml`](../.github/workflows/pages.yml).

1. In the repository, open **Settings → Pages** and set **Source** to **GitHub Actions** (one-time setup).
2. Push a change under `website/` to `main`, or run the workflow manually from the **Actions** tab.
3. The workflow uploads the `website/` folder with `actions/upload-pages-artifact` and publishes it with `actions/deploy-pages`. The site appears at `https://<user>.github.io/yonder/`.

All links are relative, so the site works both from the `/yonder/` project path and from a custom domain. To use a custom domain, add a `CNAME` file containing the domain to `website/` and configure DNS as described in the GitHub Pages docs.

## Editing notes

- Colors, fonts and radii are CSS custom properties at the top of `assets/styles.css`; dark mode overrides them in a `prefers-color-scheme: dark` block.
- Elements with the `reveal` class fade in on scroll. Animations are disabled when the visitor prefers reduced motion.
- Download buttons carry `data-download`; `main.js` points them at the latest GitHub release and labels them with the visitor's OS.
