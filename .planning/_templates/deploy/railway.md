---
platform: railway
best-for: Full-stack apps, apps with databases, APIs
free-tier: limited (trial credits)
---

# Deploy: Railway

**Install:** `npm i -g @railway/cli` (if not installed)
**Deploy command:** `railway up`
**Required:** Railway account, project linked via `railway link`
**End condition:** `railway up` exits 0, URL returns HTTP 200
**Notes:** Auto-detects framework. Good for full-stack apps with databases. Provisions PostgreSQL/Redis with one click.
**Env vars:** Set via `railway variables set KEY=VALUE` or Railway dashboard.
