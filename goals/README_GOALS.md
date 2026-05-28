# Terminal Commander Goal Files

Frozen 2026-05-28 as historical record. Live status tracked in `.agent/goals/terminal-commander-runtime/` and `docs/release/MVP_EVIDENCE_REVIEW.md`. Do not edit.

Copy the `.agent/` directory from this archive into the root of your repository:

```bash
cd <target-repo>
cp -R /path/to/extracted/.agent .
```

Then run:

```bash
/goal .agent/goals/terminal-commander-mvp/TC01-research-product-baseline-and-source-map.md
```

Branch policy inside every goal:

```text
target_branch: feature/terminal-commander-mvp
prohibited_branches: ["main", "master"]
```
