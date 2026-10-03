# Resume guide: humm after the Paperclip shutdown

Digikal ran this repository's work through Paperclip until the shutdown cutoff of 2026-10-03T06:12:00Z. This folder preserves the task history, plans, decisions, reviews and evidence so that a normal clone is enough to resume without Paperclip. It is archival documentation; it is not a plan and it authorises no work.

Snapshot: Paperclip records exported from 2026-10-03T17:39:09Z (every shutdown-era record up to then, after the original cutoff); Git refs read at 2026-10-03T17:41:58Z; source rows from ReleaseEngineer's final manifest `50db32f32fa7856c4a1699de1b3534f7a56cc573`.

## Where this folder lives

This folder is on branch `docs/paperclip-shutdown-2026-10-03` and reaches `main` when its pull request merges after review. Until then, or to read it exactly as archived:

```sh
git clone --single-branch -b docs/paperclip-shutdown-2026-10-03 https://repo.digikal.com/jawad/humm.git humm-archive
git -C humm-archive rev-parse HEAD   # compare with the final head in the infrastructure coordination.md
cd humm-archive/docs/paperclip/shutdown-2026-10-03
python3 tools/check_offline.py . --complete
```

`--complete` fails unless every hash manifest in `data/` can be checked. Without `--complete` the checker reports skipped manifests and does not claim a complete check. The checker also fails on any erased record citation that is not listed with a reason in `data/unresolved-links.tsv`, and on any attachment location in `data/issues.json` that does not hold the recorded bytes. This archive has no attachments of its own, so there is no `sha256sum` step.

## Start here

1. [planned-work.md](planned-work.md): what was unfinished, planned or shelved, and the two kinds of freeze.
2. [refs.md](refs.md): ReleaseEngineer's source rows, archive refs, pull requests and exact restore commands.
3. [decisions.md](decisions.md): founder and DevGuru decisions, blockers, review verdicts and board interactions.
4. [setup.md](setup.md): setup and test commands quoted from this repository's own docs.
5. [inventory.md](inventory.md): every archived issue with status at cutoff and at export, counts, exclusions and redactions.
- `issues/DIG-<n>.md`: one page per issue: description and acceptance criteria, every document (all revisions in `DIG-<n>.revisions.md`), attachments, interactions, every comment and the agent runs. Paperclip links inside them point at these files; `data/` holds the same facts as JSON.

## At a glance

- Remote: https://repo.digikal.com/jawad/humm.git; default branch `main` at `582b4c1da7c123c81d9bc71210c2c0d5b89bec0c`.
- Paperclip project: humm; configured working ref `main`.
- Primary issues: 2 (at cutoff: done 2; at export: done 2).
- Unfinished at cutoff or export: 0; founder-prohibited planned tasks: 0.
- Cross-reference copies from other projects: 25.
- Attachments: 0 here and 0 large files on branch `archive/paperclip-shutdown-2026-10-03-large-attachments`, all SHA-256 checked.

## Rules that still apply

- The nine todo/backlog tasks at the cutoff are founder-prohibited: none may be implemented without a new founder decision.
- Honcho is shelved (2026-10-03T14:13:56Z); restarting it needs an explicit founder decision to unshelve.
- A preserved or backup branch is not accepted work. Only a DevGuru review verdict recorded on the issue accepts work ([DIG-250](issues/DIG-250.md)).
- No earlier completion permission, recovery card or cleared blocker authorises continuing shelved or frozen work ([DIG-327](issues/DIG-327.md), [DIG-333](issues/DIG-333.md#document-shelving-disposition)).
- Never commit credentials. Setup needs (tokens, database URLs, provider keys) are placeholders in [setup.md](setup.md); the founder kept the onboarding token unchanged for now ([decision](issues/DIG-332.md#document-credential-decision)).

## Next actions

1. Read [planned-work.md](planned-work.md) and confirm with the founder which item, if any, resumes.
2. Recover the item's branch with [refs.md](refs.md) and check its latest review document before building on it.
3. Run the repository's checks from [setup.md](setup.md) on the recovered head before claiming anything works.

## Company knowledge base

- Canonical repository: `https://repo.digikal.com/jawad/digikal-knowledge-base.git`, default branch `main` at `7dc917c3324a18354613e38fa36138cbdc747fc9` (verified with `git ls-remote` at 2026-10-03T17:41:58Z). Start at `knowledge/README.md`; this project's page is `knowledge/projects/humm/README.md`. Its `records/` tree holds the issue records the knowledge base cites, so it also reads without Paperclip.
- Provenance snapshot: the 134-file knowledge base as it stood before the repository existed is on infrastructure branch `archive/shutdown-2026-10-03/off-repo/company-knowledge-and-ceo-notes` at `792a34a14f0cdb70218b63664fb422c7f2e14a69` under `onboarding/knowledge/`. CEO compared all 134 files with the live tree on 2026-10-03 and found none missing or different ([DIG-332 knowledge-storage](issues/DIG-332.md#document-knowledge-storage)). Use it only to check history.

```sh
git clone https://repo.digikal.com/jawad/digikal-knowledge-base.git && cd digikal-knowledge-base && git rev-parse HEAD   # then open knowledge/README.md
# provenance snapshot only:
git clone --single-branch -b archive/shutdown-2026-10-03/off-repo/company-knowledge-and-ceo-notes https://repo.digikal.com/jawad/infrastructure.git kb-snapshot
git -C kb-snapshot checkout 792a34a14f0cdb70218b63664fb422c7f2e14a69   # open onboarding/knowledge/README.md
```

## Company-wide context

The complete coordination index for all 345 company issues and all 18 archived repositories, with each archive's final head, is `docs/paperclip/shutdown-2026-10-03/coordination.md` on branch `docs/paperclip-shutdown-2026-10-03` of the infrastructure repository: https://repo.digikal.com/jawad/infrastructure/src/branch/docs/paperclip-shutdown-2026-10-03/docs/paperclip/shutdown-2026-10-03/coordination.md.
