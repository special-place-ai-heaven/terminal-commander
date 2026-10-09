# Code Review Guidelines

> This file configures Claude Code Review for this project.
> Claude will use these guidelines when reviewing PRs.

## Focus Areas
- [Add your review priorities here, e.g., "security", "performance", "type safety"]

## Required Checks
- [ ] No hardcoded credentials or API keys
- [ ] Error handling for all external API calls
- [ ] Types are explicit, no `any` without justification

## Out of Scope
- Style preferences handled by linter
- Minor variable naming (unless misleading)

## Context
[Optional: describe the codebase, key patterns, or things reviewers should know]
