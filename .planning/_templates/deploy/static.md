---
platform: static
best-for: Local-only, manual deployment later
free-tier: n/a
---

# Deploy: Static (Local Only)

**Install:** none
**Deploy command:** `npx serve dist` (or `npm run start`)
**Required:** nothing
**End condition:** `localhost:{port}` returns HTTP 200
**Notes:** For apps that don't need hosting yet. User deploys manually later. Good default when the user says "I don't want to deploy yet" or "I'll deploy later."
**Env vars:** Use `.env.local` for development.
