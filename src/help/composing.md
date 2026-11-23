How to put several roles together for one agent: which a role requires, which a project names, and where an agent started outside any repository gets its roles.

1. Requires or use. A role that every agent of another role needs ("an iOS app coder is also a coder") is named in that role's `requires`, never listed again per project. A role some projects want and others do not (for example running tests on a real phone) is not required by anything; it is named in `rolesmith use` only for the projects that want it.
2. One project per combination. An agent started outside any repository (a harness given a directory of roles, such as factorium's `--role <dir>`) gets a directory that holds only `rolesmith.toml` and the installs: `rolesmith use A B --into <target> --project <dir>`. Each combination of roles is its own directory.
3. Name it by the kind of work, not by the project that first uses it, so another project doing the same work reuses it (e.g. `ios-ml-app`, not `chessmirror-roles`).
4. Its entry file (`CLAUDE.md` / `AGENTS.md` beside the installs) lists the roles the directory installs and tells the agent to read each one's core skill `rolesmith-<role>`. Required roles are not listed: the core skill names them.
5. Changing a combination makes a new directory rather than editing one agents are running from. The old one is removed (`rolesmith remove`, then the directory) only after every agent started from it has finished.
6. New versions: `rolesmith source update <source> && rolesmith update --project <dir>`; agents started after that read the new version.
