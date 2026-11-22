# Issue tracker

Issues for this repo live in factorium's local tracker, not in GitHub Issues.

- Read and write them with `factorium issue add|list|show|comment|block rolesmith …`; `rolesmith` is
  this repo's name in `factorium checkout list`.
- Dependencies are given when an issue is filed (`--blocked-by <n>`), so a worker never picks up an
  issue before what it needs has landed.
- GitHub Issues on sunfmin/rolesmith are not this repo's tracker. (A library's GitHub issues are
  where rolesmith files *lessons*, SPEC.md §6; that is the product, not this repo's work.)
