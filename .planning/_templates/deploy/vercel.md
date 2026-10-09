---
platform: vercel
best-for: Next.js, React, static sites
free-tier: yes
---

# Deploy: Vercel

**Install:** `npm i -g vercel` (if not installed)
**Deploy command:** `vercel --prod`
**Required:** Vercel account, project linked via `vercel link`
**End condition:** `vercel --prod` exits 0, URL returns HTTP 200
**Notes:** Automatic for Next.js projects. For other frameworks, may need `vercel.json`.
**Env vars:** Set via `vercel env add` or Vercel dashboard. Never commit `.env`.
