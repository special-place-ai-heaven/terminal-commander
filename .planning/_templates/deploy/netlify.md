---
platform: netlify
best-for: Static sites, JAMstack, React SPAs
free-tier: yes
---

# Deploy: Netlify

**Install:** `npm i -g netlify-cli` (if not installed)
**Deploy command:** `netlify deploy --prod --dir=dist`
**Required:** Netlify account, site linked via `netlify link`
**End condition:** `netlify deploy` exits 0, URL returns HTTP 200
**Notes:** Needs build output directory. Usually `dist/`, `.next/`, or `build/`.
**Env vars:** Set via `netlify env:set KEY VALUE` or Netlify dashboard.
