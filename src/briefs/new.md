# Draft the role `{role}`

A person asked for a new role, and you are to draft it. rolesmith has made the branch `{branch}`
in the working copy of the library `{source}` and scaffolded `roles/{role}/` there:

    {dir}

## The need

{need}

## What a role is

A role does one kind of work: ship an app to the App Store, write a Rust CLI, test an iOS UI. It
carries what an agent needs for that work: its core memory, its skills, the programs it runs and the
services it may reach. Every agent that uses the role reads its core memory on every task, and loads
a skill only when the skill's description fits the task at hand.

## Before you write

Research how practitioners do this work well: the official documentation of the tools involved,
the guides and talks of people known for doing it, the failures they warn about. Keep the sources:
put the link beside each rule it supports, in `core.md` or the skill, so a reviewer can check it.
Prefer a few rules that a careful practitioner would insist on over a long list of generalities;
write what an agent would otherwise get wrong.

## The files you may change, and what `rolesmith check` enforces in them

- `roles/{role}/role.toml`: a `description` of the work, one line of at most {max_description}
  characters (the need is there now as a draft: rewrite it); `programs`, tools of `tools/` in the
  same library by name; `services`, the mytokens entries the work reads, by name. No other
  top-level key besides `[match]` and the tables of other consumers ({consumer_tables}).
- `roles/{role}/core.md`: the role's standing memory, rules that hold for every task of the role,
  at most {max_core} characters. Anything that applies only to some tasks belongs in a skill.
- `roles/{role}/prohibitions.toml`: what the role must never do, each `[[prohibition]]` with a
  `rule` and a `why`.
- `roles/{role}/skills/<skill>/SKILL.md`: knowledge for some tasks, with YAML front matter `name`
  (equal to the directory name) and `description` (at most {max_skill_description} characters,
  saying when to use it). Files beside `SKILL.md` (scripts, references) are copied with it.
- `tools/<tool>/tool.toml`, only to add a tool the role's `programs` names that the library lacks;
  its `[install]` pins a `brew` formula and a `version`.

Role and skill names are lowercase ASCII letters, digits and single hyphens, 2 to {max_name}
characters. No file may hold text that looks like a secret: name a credential by its mytokens
entry, never its value. Change nothing else in the library, and do not commit: `rolesmith propose`
does. `check` cannot judge whether the role is any good: that is your research and the person's
review.

## The last step

In `{dir}`:

    rolesmith check {role}
    rolesmith propose {role}

Fix every problem `check` reports and run it again until it passes; then `propose` commits the
role on `{branch}`, pushes it and opens the pull request a person merges.
