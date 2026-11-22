# Distill the open lessons of `{role}`

A person asked you to fold the open lessons of the role `{role}` into it. rolesmith has put the
working copy of the library `{source}` on the new branch `{branch}`, made from `main`:

    {dir}

The role is in `roles/{role}/` there. Its lessons are issues on `{repo}`; {count} are open, below.

## How to distill

Take the lessons one at a time. For each, decide what the role should know so that what happened
does not happen again, and write it in the narrowest place it belongs:

- a skill, when it matters only for some tasks: the skill whose description fits those tasks, or a
  new one under `roles/{role}/skills/` when none does;
- `prohibitions.toml`, when it is something the role must never do;
- `core.md` only for a rule of every task the role does. Every agent reads core on every task, so
  each line there costs every task; prefer a skill.

Fold a lesson into the text that is there: sharpen or replace a rule it corrects rather than add a
second rule beside it. Keep the lesson's evidence link beside the rule it supports, so a reviewer
can check it. Write project details (customer names, private URLs) in general terms.

A lesson that is wrong, already covered, or not this role's to know is not absorbed: close it with
the reason,

    rolesmith lesson close {source}#<n> --why "<why it will not be absorbed>"

and leave the role as it is for it.

## The files you may change, and what `rolesmith check` enforces in them

- `roles/{role}/core.md`: at most {max_core} characters.
- `roles/{role}/prohibitions.toml`: each `[[prohibition]]` with a `rule` and a `why`.
- `roles/{role}/skills/<skill>/SKILL.md`: YAML front matter `name` (equal to the directory name)
  and `description` (at most {max_skill_description} characters, saying when to use it). Files
  beside `SKILL.md` are copied with it.
- `roles/{role}/role.toml`, only when a lesson shows the description, `programs` or `services` are
  wrong; its `description` is one line of at most {max_description} characters.

Skill names are lowercase ASCII letters, digits and single hyphens, 2 to {max_name} characters. No
file may hold text that looks like a secret: name a credential by its mytokens entry, never its
value. Change nothing outside `roles/{role}/`, and do not commit: `rolesmith propose` does.

## The role now

### core.md

{core}

### Prohibitions

{prohibitions}

### Skills

{skills}

## The open lessons

{lessons}

## The last step

In `{dir}`:

    rolesmith check {role}
    rolesmith propose {role} --closes <n> ...

Fix every problem `check` reports and run it again until it passes. Give `--closes` the number of
each lesson you absorbed (not those you closed with `lesson close`); `propose` commits the change on
`{branch}`, pushes it and opens the pull request that closes them when a person merges it.
