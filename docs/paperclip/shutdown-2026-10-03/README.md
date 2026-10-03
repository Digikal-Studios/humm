# Resume guide: humm after the Paperclip shutdown

Digikal ran this repository's work through Paperclip until the shutdown cutoff of 2026-10-03T06:12:00Z. This folder preserves the task history so that a normal clone is enough to resume without Paperclip. It is archival documentation; it is not a plan and it authorises no work.

Snapshot: baseline export 2026-10-03T07:00Z (a final delta follows after source preservation [DIG-334](issues/DIG-334.md) and in-flight disposition [DIG-333](issues/DIG-333.md) finish).

## Start here

1. [planned-work.md](planned-work.md): what was unfinished or planned in this repository at the cutoff.
2. [refs.md](refs.md): every branch, SHA and pull request, and how to recover them.
3. [decisions.md](decisions.md): blockers, review verdicts and board decisions.
4. [setup.md](setup.md): setup and test commands quoted from this repository's own docs.
5. [inventory.md](inventory.md): every archived issue with status at cutoff and now, counts and exclusions.
6. `issues/DIG-<n>.md`: one page per issue with description, acceptance criteria, documents, attachments, interactions and every comment. `data/` holds the same facts as JSON.

## At a glance

- Remote: https://repo.digikal.com/jawad/humm.git; default branch `main` at `086426996bc81113fc4da920e199a9943bd33603`.
- Paperclip project: humm; configured working ref `main`.
- Primary issues: 2 (done 2 at cutoff).
- Unfinished or planned primary issues: 0; frozen planned tasks: 0.
- Cross-reference copies from other projects: 21.
- Attachments copied: 0 here and 0 large files on branch `archive/paperclip-shutdown-2026-10-03-large-attachments` (SHA-256 verified).

## Rules that still apply

- Planned (todo/backlog) work stays frozen until the founder decides to resume it.
- A preserved or backup branch is not accepted work. Only a DevGuru review verdict recorded on the issue accepts work ([DIG-250](issues/DIG-250.md)).
- Board recovery holds on [DIG-299](issues/DIG-299.md) and [DIG-305](issues/DIG-305.md) stay in force; see [DIG-327](issues/DIG-327.md).
- Never commit credentials. Setup needs (tokens, database URLs, provider keys) are described as placeholders in [setup.md](setup.md).

## Next actions

1. Read [planned-work.md](planned-work.md) and confirm with the founder which item, if any, resumes.
2. Recover the item's branch with [refs.md](refs.md) and check its latest review document before building on it.
3. Run the repository's checks from [setup.md](setup.md) on the recovered head before claiming anything works.

## Company-wide context

The complete coordination index for all 335 company issues and all 18 repositories lives in the infrastructure repository: https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/coordination.md.
