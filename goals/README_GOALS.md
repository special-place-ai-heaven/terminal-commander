# Terminal Commander Goal Files

**Historical MVP chain (frozen).** `goals/TC01–TC32` statuses were reconciled in `docs/release/MVP_EVIDENCE_REVIEW.md` ([ROB-4](mention://issue/1d99ebb1-c568-48dd-85e5-a0f70e0dfe69)). New work is tracked only under `.agent/goals/terminal-commander-runtime/`.

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
