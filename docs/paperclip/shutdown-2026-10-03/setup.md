# Setup and test commands

Quoted verbatim from this repository's existing documentation at `main` commit `086426996bc81113fc4da920e199a9943bd33603`. This page adds no new commands; when the source file changes, the source file wins. Commands that need credentials or hosts read them from the environment or a secrets store; this archive contains no secret values.

No setup or test commands are documented in README.md, AGENTS.md, CLAUDE.md, CONTRIBUTING.md or a Makefile on the default branch. Read the repository's top-level files and the issue pages in this archive for how work was verified.

## Required setup that is not in Git

- Remote access: clone over HTTPS from `repo.digikal.com` with your own account or token. No token is recorded here.
- Any `.env`, database URL, provider API key or deployment credential named in the commands above must be supplied by the operator. Use the repository's own example files (for example `.env.example` or `.env.template`) where they exist.
