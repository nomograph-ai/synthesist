---
id: auth-api-keys
title: Add API key authentication to REST endpoints
status: approved
created: 2026-03-08
updated: 2026-03-08
author: plan
---

<goal>
Allow external services to authenticate via API keys in the Authorization header,
alongside the existing JWT bearer token flow. API keys are stored hashed in the
database, scoped to a user, and can be revoked without affecting the user's session.
</goal>

<context>
- path: src/middleware/auth.ts
  why: Current auth middleware — only supports JWT. Must be extended.
- path: src/models/user.ts
  why: User model we'll associate API keys with.
- path: src/routes/index.ts
  why: Route registration — new /api/keys endpoints go here.
- path: src/db/migrations/
  why: Migration directory for the new api_keys table.
</context>

<constraints>
- Must not break existing JWT authentication flow
- API keys must be stored as bcrypt hashes, never plaintext
- Key creation returns the raw key exactly once; it cannot be retrieved after
- Must support key revocation without database deletion (soft revoke)
- All new endpoints must return proper 401/403 responses
- No new runtime dependencies beyond what's in package.json
</constraints>

<decisions>
- question: API key format?
  answer: Prefix + random bytes — "sk_live_" + 32 hex chars
  rationale: Prefix makes keys greppable in logs/code; 128 bits of entropy is sufficient

- question: Rate limiting per API key?
  answer: Not in this spec — separate feature
  rationale: Keep scope bounded; rate limiting touches infrastructure

- question: Store in separate table or add column to users?
  answer: Separate api_keys table with user_id foreign key
  rationale: Users may have multiple keys; need revocation timestamps per key
</decisions>

<discovery>
- finding: Auth middleware uses a strategy pattern (src/middleware/auth.ts:23)
  impact: Can add ApiKeyStrategy without modifying existing JwtStrategy
  action: t1 creates the strategy; t2 registers it in the middleware chain
  agent: plan
  date: 2026-03-08
</discovery>
