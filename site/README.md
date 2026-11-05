# Landing site

Independent static Astro site. The screenshots use staged open-film data and generated sample clips. The landing page never connects to local files, installed apps, or devices.

## Development

Run inside `site/`:

```sh
bun install --frozen-lockfile
bun run dev
bun run verify
```

`verify` type-checks Astro, builds static output, and checks routes, discovery, anchors, assets, and documentation. Browser review covers desktop/mobile layouts, keyboard access, demo interactions and reset, theme switching, copy buttons, and optional-detail disclosures.

## Vercel

The dedicated Vercel project is `mls` in `pushkar-patels-projects`. GitHub Actions deploys from `site/`, so leave the Vercel **Root Directory** unset. Use **Framework: Astro**, **Install Command: `bun install --frozen-lockfile`**, **Build Command: `bun run verify`**, **Output Directory: `dist`**, and Node.js 24.

`.github/workflows/site.yml` verifies pull requests without deployment credentials. Every push to `main` runs `vercel pull`, `vercel build --prod`, and `vercel deploy --prebuilt --prod`. The build runs all site checks before publishing the resulting artifact. Production runs are serialized; manual dispatch is supported on `main`. Existing application workflows run independently. Leave Vercel Git integration disconnected to avoid duplicate deployments.

GitHub repository variables `VERCEL_ORG_ID` and `VERCEL_PROJECT_ID` identify the destination. The `VERCEL_TOKEN` repository secret is a dedicated project-scoped token. Rotate it in Vercel and replace the GitHub secret when needed; never commit credentials or the generated `.env.local` and `.vercel/` files. The public site itself requires no runtime environment variables or backend.

`vercel.json` supplies build settings and response headers. Add the production hostname from `astro.config.mjs` and `src/content.json` under Vercel Domains, then add the exact CNAME Vercel supplies at the current DNS provider.

## Content and discovery

`src/pages/index.astro` contains the concise landing flow. `src/content.json` supplies shared copy for `/index.md` and `/llms.txt`. `src/agent-guide.md` supplies the installation and usage guide at `/agents.md`; keep its commands and behavior aligned with the repository README and supported interfaces. Edit it to keep installation instructions, supported interfaces, and limitations consistent. `src/components/Demo.astro` provides the interactive preview. Capture descriptions and links are also included in the Markdown overview.

Canonical and social URLs use the production subdomain. `/robots.txt` and `/sitemap.xml` use the same host. Public production must be reachable without Vercel Authentication or browser challenges so unauthenticated people and agents can read it. Builds with `VERCEL_ENV=preview` emit a `noindex` meta tag; Vercel also supplies a noindex response header on preview URLs. Preview access can remain protected.

Search and social titles, social image descriptions, and supported operating systems live in `src/content.json`. `PageHead.astro` emits Open Graph, Twitter cards, and SoftwareApplication JSON-LD with author and product screenshots. Keep `public/social.svg` and its 1200 × 630 PNG export synchronized with the headline. `verify` checks the generated metadata, PNG dimensions, and environment-specific indexing policy.

After deployment, check the custom-domain homepage, `/social.png`, `/index.md`, `/agents.md`, `/llms.txt`, `/robots.txt`, and `/sitemap.xml` without authentication. Confirm the production response has no noindex directive and preview responses do. Submit the sitemap in Search Console and link the production site from the repository README and portfolio. These external launch steps require the public domains to be live.

The screenshots use actual application UI with staged data. Official Blender Studio posters are licensed CC BY 4.0; source links, license links, creator credits, and usage notes are in the footer and `public/artwork/ATTRIBUTION.txt`. The site makes no third-party font or analytics requests. Product documentation is authoritative for the installed version.

## Updating terminal captures

On macOS with `mls`, `ffmpeg`, and `uv` installed, run:

```sh
uv run scripts/capture-terminal.py
```

The script generates 12-second sample videos from licensed poster artwork and synthetic WAV/FLAC audio in a temporary directory and runs the installed MLS binary in a 120 × 32 virtual terminal. It preserves the emitted terminal cells, ANSI colors, and half-block thumbnails, using Menlo for rendering. It captures the file browser, with a matching text transcript, under `public/screenshots/`. Terminal colors and image protocols can vary by terminal. The capture maps ANSI blue to a readable terminal palette. `public/samples.json` contains actual JSON excerpts with machine paths and the response envelope omitted.

The included captures use MLS 0.0.3. Install the version you intend to showcase before regenerating. These capture-only Python dependencies and the MLS executable are not needed for Vercel builds.

## Appearance

One icon button cycles System (default) → Light → Dark → System. Its icon shows the current mode; its tooltip and accessible label identify the current mode and next action. CSS `color-scheme` and `light-dark()` follow the OS automatically in System mode, including without JavaScript. An explicit choice is applied before paint and saved in local storage; resetting to System removes the override. TV Cast and IINA demo frames follow the effective page theme. The MLS terminal retains its dark terminal palette.

The product layouts are independent. Shared utilities cover metadata, theme selection, copy controls and attribution. Keep user-facing product copy and the Markdown content in `src/content.json` consistent when changing the page.

## Mobile preview

Screens up to 900px wide and touch devices without hover show a static desktop capture. The image keeps its original proportions and follows System, Light, or Dark appearance. Interactive demo initialization is deferred until a larger, hover-capable viewport is available.

`src/components/MobilePreview.astro` selects `public/screenshots/demo-desktop-light.png` or `demo-desktop-dark.png`. Capture the complete desktop demo with staged data after artwork loads, and keep both images at the same dimensions. The image dimensions in `Demo.astro` must match the PNGs; `verify` checks this contract.

## Interactive preview

The preview recreates the MLS terminal in a Mac terminal window. Navigate folders with arrows or h/j/k/l and Enter; use / to filter, s/S to sort, 1–5 for kind filters, Space to mark, i for metadata, and p/P and [/] for mock playback. q exits to a sample prompt; running mls opens the browser again. Music includes WAV and FLAC metadata from real MLS output. File selection updates the JSON example. No local files are inspected or modified.

The browser demo is implemented in `src/components/Demo.astro`. `public/samples.json` is its metadata source; its desktop layout follows the three-pane Rust TUI. The captured native terminal remains linked from the Markdown guide.
