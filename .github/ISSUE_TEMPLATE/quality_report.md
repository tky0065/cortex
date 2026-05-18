---
name: Generated project quality report
about: Report poor quality, inconsistent, or incomplete output from a Cortex workflow
labels: quality, generated-output
assignees: ''
---

## Summary

What was wrong with the generated project?

## Command used

```bash
cortex start "..." --auto --workflow dev
```

## Environment

- Cortex version: (`cortex --version`)
- OS:
- Provider:
- Model:
- Workflow: dev / code-review / marketing / prospecting / custom

## Expected quality

What would a good output look like? (e.g. "a working Rust CLI with tests and a Dockerfile")

## Actual quality

What did Cortex produce instead? Describe the specific problem:

- [ ] Missing files (list them)
- [ ] Build fails in generated project
- [ ] Tests fail or are missing
- [ ] Dockerfile invalid or missing
- [ ] README missing or wrong instructions
- [ ] Specs / architecture don't match generated code
- [ ] Repeated or contradictory content
- [ ] TODO / placeholder code left in output
- [ ] Other

## Generated project structure

Paste the output of `find <project-dir> -type f` or a tree listing.

```
<paste here>
```

## Error output (if any)

```
<paste build/test/lint output here>
```

## Eval checker result (if you ran it)

```bash
evals/check_dev_output.sh <project-dir>
```

```
<paste output here>
```

## Additional context

Any custom agents, custom workflows, or unusual config involved?
