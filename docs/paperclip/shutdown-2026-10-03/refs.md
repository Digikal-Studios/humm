# Branch and commit inventory

Remote `https://repo.digikal.com/jawad/humm.git`; default branch `main` at `582b4c1da7c123c81d9bc71210c2c0d5b89bec0c` (refs read 2026-10-03T17:41:58Z). Paperclip's configured ref for this project: `main`.

A branch or commit listed here is preserved source, not accepted work. Acceptance is recorded only by the review documents on the issue pages.

## Source-preservation manifest (final)

ReleaseEngineer's final manifest ([DIG-338](issues/DIG-338.md#document-source-preservation), building on [DIG-334](issues/DIG-334.md#document-source-preservation)) is infrastructure branch `archive/shutdown-2026-10-03/source-preservation-manifest` at `50db32f32fa7856c4a1699de1b3534f7a56cc573`. It audited every Git checkout on the Paperclip host at 16:05–16:09Z: 326 rows in 19 repositories, 230 of 230 restore checks OK, every commit on a remote, no held exception left. Its `README.md` and `final-delta.md` explain the method; `restore-verify.tsv` holds the restore results.

```sh
git clone --single-branch -b archive/shutdown-2026-10-03/source-preservation-manifest https://repo.digikal.com/jawad/infrastructure.git manifest
git -C manifest checkout 50db32f32fa7856c4a1699de1b3534f7a56cc573
less manifest/final-delta.md manifest/per-repo/humm.tsv manifest/restore-verify.tsv
```

### Source rows for humm (3 of 7)

Rows in the classes `managed-project-clone`, `task-worktree` and `agent-workspace-checkout`. The other classes are archive or verification copies (this docs branch, ReleaseEngineer's build clones) and are in the manifest file. *Original* is the SHA on the host; *remote* is the named remote ref that contains it. Run each restore command inside a fresh full clone of this repository.

| Issue | Owner now | Location (class) | Kind | Original ref | Original SHA | On remote via | State | Restore check | Restore command |
|---|---|---|---|---|---|---|---|---|---|
| — | unknown (shared clone; no issue in ref or path) | `humm` (managed-project-clone) | branch | `main` | `086426996bc81113fc4da920e199a9943bd33603` | yes, `refs/heads/main` @ `582b4c1da7c1` | - | OK | `git switch -c restore/main 086426996bc81113fc4da920e199a9943bd33603 && test "$(git rev-parse HEAD)" = 086426996bc81113fc4da920e199a9943bd33603` |
| — | unknown (shared clone; no issue in ref or path) | `humm` (managed-project-clone) | otherref | `refs/tags/v1.0.0` | `dd54e609932f07b6b6bfcfdd21460393c1d9a1c9` | yes, `refs/tags/v1.0.0` @ `dd54e609932f` | - | OK | `git switch -c restore/v1.0.0 dd54e609932f07b6b6bfcfdd21460393c1d9a1c9` |
| — | unknown (shared clone; no issue in ref or path) | `humm` (managed-project-clone) | worktree | `main` | `086426996bc81113fc4da920e199a9943bd33603` | yes, `refs/heads/main` @ `582b4c1da7c1` | staged=0 modified=0 untracked=0 ignored_local_only=0 | OK | `git switch -c restore/humm 086426996bc81113fc4da920e199a9943bd33603 && test "$(git rev-parse HEAD)" = 086426996bc81113fc4da920e199a9943bd33603` |

Full host paths for each row are in the manifest's `per-repo/humm.tsv`.

### Archive refs (0)

None for this repository.

## Knowledge-base link (DIG-340)

`main` at `582b4c1da7c123c81d9bc71210c2c0d5b89bec0c` includes the merged knowledge-base entry point; task branch `paperclip/DIG-340` is retained at `705ff5c4ba50377026c0149441ed51417d3b8018` ([DIG-340 project-adoption](issues/DIG-340.md#document-project-adoption)).

## Recover any branch

```sh
git clone https://repo.digikal.com/jawad/humm.git
cd humm
git fetch origin '+refs/heads/*:refs/remotes/origin/*' '+refs/tags/*:refs/tags/*'
git switch -c <local-name> origin/<branch>   # branch names below
git cat-file -e <sha>^{commit} && echo present   # check a SHA from an issue page
```

## Branch and commit evidence recorded on issues

| Issue | Type | Branch | SHA | URL | Remote state |
|---|---|---|---|---|---|
| [DIG-299](issues/DIG-299.md) | pull_request | `` | `` | https://repo.digikal.com/jawad/honcho/pulls/29 | — |
| [DIG-305](issues/DIG-305.md) | pull_request | `` | `` | https://repo.digikal.com/jawad/honcho/pulls/22 | — |
| [DIG-332](issues/DIG-332.md) | pull_request | `` | `` | https://repo.digikal.com/jawad/infrastructure/pulls/3 | — |
| [DIG-334](issues/DIG-334.md) | commit | `archive/shutdown-2026-10-03/source-preservation-manifest` | `2fa1e5f1ea56c73f5d84f2da110e271917f10ed8` | https://repo.digikal.com/jawad/infrastructure/commit/2fa1e5f1ea56c73f5d84f2da110e271917f10ed8 | not a remote branch head (merged or only in history; see the manifest rows above) |
| [DIG-334](issues/DIG-334.md) | commit | `archive/shutdown-2026-10-03/source-preservation-manifest` | `0a6102fcbee8a966ac21490b9eba06ebffebba3f` | https://repo.digikal.com/jawad/infrastructure/commit/0a6102fcbee8a966ac21490b9eba06ebffebba3f | not a remote branch head (merged or only in history; see the manifest rows above) |
| [DIG-334](issues/DIG-334.md) | branch | `archive/shutdown-2026-10-03/source-preservation-manifest` | `51333a7ee291b2935b32c551d7f4ebea55467dd5` | https://repo.digikal.com/jawad/infrastructure/src/branch/archive/shutdown-2026-10-03/source-preservation-manifest | not a remote branch head (merged or only in history; see the manifest rows above) |
| [DIG-334](issues/DIG-334.md) | branch | `archive/shutdown-2026-10-03/off-repo/honcho-dig-97-logs` | `ed24d780016b9bf08ba7e09db1337992fc34c95d` | https://repo.digikal.com/jawad/honcho/src/branch/archive/shutdown-2026-10-03/off-repo/honcho-dig-97-logs | not a remote branch head (merged or only in history; see the manifest rows above) |
| [DIG-334](issues/DIG-334.md) | branch | `archive/shutdown-2026-10-03/off-repo/nurikabe-playtest-dig46-tools` | `c576211a04bd7b0283b9a55aa50a5cfaf90e9ae0` | https://repo.digikal.com/jawad/nurikabe/src/branch/archive/shutdown-2026-10-03/off-repo/nurikabe-playtest-dig46-tools | not a remote branch head (merged or only in history; see the manifest rows above) |
| [DIG-334](issues/DIG-334.md) | branch | `archive/shutdown-2026-10-03/off-repo/company-knowledge-and-ceo-notes` | `792a34a14f0cdb70218b63664fb422c7f2e14a69` | https://repo.digikal.com/jawad/infrastructure/src/branch/archive/shutdown-2026-10-03/off-repo/company-knowledge-and-ceo-notes | not a remote branch head (merged or only in history; see the manifest rows above) |
| [DIG-334](issues/DIG-334.md) | branch | `archive/shutdown-2026-10-03/off-repo/dig-59-report` | `6bca41840f4c5ee29c646258e17d6a4207deaf59` | https://repo.digikal.com/jawad/production-ready-engineering-skill/src/branch/archive/shutdown-2026-10-03/off-repo/dig-59-report | not a remote branch head (merged or only in history; see the manifest rows above) |
| [DIG-334](issues/DIG-334.md) | branch | `archive/shutdown-2026-10-03/unfinished/honcho-project-clone-wip` | `694b5a84e4b2109ad5b79ac196c639181b1894c0` | https://repo.digikal.com/jawad/honcho/src/branch/archive/shutdown-2026-10-03/unfinished/honcho-project-clone-wip | not a remote branch head (merged or only in history; see the manifest rows above) |
| [DIG-334](issues/DIG-334.md) | branch | `archive/shutdown-2026-10-03/unfinished/nurikabe-project-clone-wip` | `ce0a8246dae2f8e0c90f09f225c2bf7c4b47e6c1` | https://repo.digikal.com/jawad/nurikabe/src/branch/archive/shutdown-2026-10-03/unfinished/nurikabe-project-clone-wip | not a remote branch head (merged or only in history; see the manifest rows above) |
| [DIG-334](issues/DIG-334.md) | branch | `archive/shutdown-2026-10-03/superseded/nurikabe-dig-130-desk-720x560` | `25c04432a68306e24b76595a691b3da2ac52b87d` | https://repo.digikal.com/jawad/nurikabe/src/branch/archive/shutdown-2026-10-03/superseded/nurikabe-dig-130-desk-720x560 | not a remote branch head (merged or only in history; see the manifest rows above) |
| [DIG-334](issues/DIG-334.md) | branch | `archive/shutdown-2026-10-03/off-repo/gameplay-workspace-scripts` | `9db8b4807a2dd293fb81a73a89538992946bd115` | https://repo.digikal.com/jawad/nurikabe/src/branch/archive/shutdown-2026-10-03/off-repo/gameplay-workspace-scripts | not a remote branch head (merged or only in history; see the manifest rows above) |
| [DIG-334](issues/DIG-334.md) | branch | `archive/shutdown-2026-10-03/unfinished/sweeper-stash-0-dig-111` | `0f6e30d3d292b55a0908940eb4e4ad52b8e1859b` | https://repo.digikal.com/jawad/sweeper/src/branch/archive/shutdown-2026-10-03/unfinished/sweeper-stash-0-dig-111 | not a remote branch head (merged or only in history; see the manifest rows above) |
| [DIG-334](issues/DIG-334.md) | branch | `archive/shutdown-2026-10-03/unfinished/sweeper-project-clone-wip` | `41c69e42988605019c10365cb0bd9cd7bc15eef2` | https://repo.digikal.com/jawad/sweeper/src/branch/archive/shutdown-2026-10-03/unfinished/sweeper-project-clone-wip | not a remote branch head (merged or only in history; see the manifest rows above) |
| [DIG-334](issues/DIG-334.md) | branch | `archive/shutdown-2026-10-03/off-repo/dig-44-palette` | `ce67691ef45b337c5df0b52cc0d5b443f6db20ed` | https://repo.digikal.com/jawad/sweeper/src/branch/archive/shutdown-2026-10-03/off-repo/dig-44-palette | not a remote branch head (merged or only in history; see the manifest rows above) |
| [DIG-335](issues/DIG-335.md) | branch | `archive/paperclip-shutdown-2026-10-03-large-attachments` | `432525e9a4a4f024a9cde4d095d5afdf6bbe1ad6` | https://repo.digikal.com/jawad/nurikabe/src/branch/archive/paperclip-shutdown-2026-10-03-large-attachments | not a remote branch head (merged or only in history; see the manifest rows above) |
| [DIG-335](issues/DIG-335.md) | pull_request | `docs/paperclip-shutdown-2026-10-03` | `13695eac8d547dceca8750be8895b2816855f013` | https://repo.digikal.com/jawad/production-ready-engineering-skill/pulls/1 | branch on remote |
| [DIG-335](issues/DIG-335.md) | pull_request | `docs/paperclip-shutdown-2026-10-03` | `8082a8d0f058b3ecde89210aa07cf62d349bab3f` | https://repo.digikal.com/jawad/n8n/pulls/1 | branch on remote |
| [DIG-335](issues/DIG-335.md) | pull_request | `docs/paperclip-shutdown-2026-10-03` | `e027b27428f81099f317db5f984a7dcec2605217` | https://repo.digikal.com/jawad/j-bot/pulls/1 | branch on remote |
| [DIG-335](issues/DIG-335.md) | pull_request | `docs/paperclip-shutdown-2026-10-03` | `ba63b8073750f2f4424d99fc37b9c2e44b9b37d3` | https://repo.digikal.com/jawad/honcho/pulls/26 | branch on remote |
| [DIG-335](issues/DIG-335.md) | pull_request | `docs/paperclip-shutdown-2026-10-03` | `ccdd33b4c1e8f2f142f2a6f275b5bd7dd6b579ef` | https://repo.digikal.com/jawad/clear-technical-communication/pulls/1 | branch on remote |
| [DIG-335](issues/DIG-335.md) | pull_request | `docs/paperclip-shutdown-2026-10-03` | `2d934d1fa012148570321e9aa2e43e51de9cc043` | https://repo.digikal.com/jawad/SkillOpt/pulls/1 | branch on remote |
| [DIG-335](issues/DIG-335.md) | pull_request | `docs/paperclip-shutdown-2026-10-03` | `da2fd0a2e88c5ca43eabfe251d4b8f27954d6f81` | https://repo.digikal.com/jawad/tvi/pulls/10 | branch on remote |
| [DIG-335](issues/DIG-335.md) | pull_request | `docs/paperclip-shutdown-2026-10-03` | `9f5a6bc5dfd984f30e583fa6ce08c74f0c59b10d` | https://repo.digikal.com/jawad/sweeper/pulls/8 | branch on remote |
| [DIG-335](issues/DIG-335.md) | pull_request | `docs/paperclip-shutdown-2026-10-03` | `fe8aa8246e910dc79031973d6b40510282da3598` | https://repo.digikal.com/jawad/jsync/pulls/1 | branch on remote |
| [DIG-335](issues/DIG-335.md) | pull_request | `docs/paperclip-shutdown-2026-10-03` | `548ed01f7095630fe14fec2142211bc37f9f269c` | https://repo.digikal.com/jawad/zimam/pulls/1 | branch on remote |
| [DIG-335](issues/DIG-335.md) | pull_request | `docs/paperclip-shutdown-2026-10-03` | `00e5fb45225018f80de28e47c7a0d9c506a36a2c` | https://repo.digikal.com/jawad/adp/pulls/1 | branch on remote |
| [DIG-335](issues/DIG-335.md) | pull_request | `docs/paperclip-shutdown-2026-10-03` | `013c62f0c10e63fd247534350ee74229d6c5bb2f` | https://repo.digikal.com/jawad/ai-engineering-governance/pulls/1 | branch on remote |
| [DIG-335](issues/DIG-335.md) | pull_request | `docs/paperclip-shutdown-2026-10-03` | `fe80e6a778e38b8a9cb10d8891b4157b1ed1f5fa` | https://repo.digikal.com/jawad/humm/pulls/1 | branch on remote |
| [DIG-335](issues/DIG-335.md) | pull_request | `docs/paperclip-shutdown-2026-10-03` | `02aa95a2a98e01d21e1bb15add6808e5168b4887` | https://repo.digikal.com/jawad/RustView/pulls/1 | branch on remote |
| [DIG-335](issues/DIG-335.md) | pull_request | `docs/paperclip-shutdown-2026-10-03` | `f2d783b46de9ce3f2d8c967f1888073897e5d938` | https://repo.digikal.com/jawad/nurikabe/pulls/87 | branch on remote |
| [DIG-335](issues/DIG-335.md) | pull_request | `docs/paperclip-shutdown-2026-10-03` | `c8a84ff713f23f9d24bd2b41ce398dbb7dda3648` | https://repo.digikal.com/jawad/browser-native-rts-poc/pulls/1 | branch on remote |
| [DIG-335](issues/DIG-335.md) | pull_request | `docs/paperclip-shutdown-2026-10-03` | `3e0a57272df65d39095df1ad95169abfc9a0b22e` | https://repo.digikal.com/jawad/aletheia/pulls/1 | branch on remote |
| [DIG-335](issues/DIG-335.md) | pull_request | `docs/paperclip-shutdown-2026-10-03` | `a2ad00ddee28e4ed6ad6db7561ed60ae81a9b192` | https://repo.digikal.com/jawad/infrastructure/pulls/2 | branch on remote |
| [DIG-338](issues/DIG-338.md) | commit | `archive/shutdown-2026-10-03/source-preservation-manifest` | `50db32f32fa7856c4a1699de1b3534f7a56cc573` | https://repo.digikal.com/jawad/infrastructure/commit/50db32f32fa7856c4a1699de1b3534f7a56cc573 | not a remote branch head (merged or only in history; see the manifest rows above) |
| [DIG-338](issues/DIG-338.md) | branch | `archive/shutdown-2026-10-03/unfinished/final-nurikabe` | `1cde3346714845ac38a31ad18419d4c76ec5f42d` | https://repo.digikal.com/jawad/nurikabe/src/branch/archive/shutdown-2026-10-03/unfinished/final-nurikabe | not a remote branch head (merged or only in history; see the manifest rows above) |
| [DIG-338](issues/DIG-338.md) | commit | `archive/shutdown-2026-10-03/source-preservation-manifest` | `5779d2e17c8d70ca2c1de9cb642342f6f59a823e` | https://repo.digikal.com/jawad/infrastructure/commit/5779d2e17c8d70ca2c1de9cb642342f6f59a823e | not a remote branch head (merged or only in history; see the manifest rows above) |
| [DIG-338](issues/DIG-338.md) | commit | `archive/shutdown-2026-10-03/source-preservation-manifest` | `feabc9fd68fb0f2a0b9cdbfc7196e0e3ac0901bd` | https://repo.digikal.com/jawad/infrastructure/commit/feabc9fd68fb0f2a0b9cdbfc7196e0e3ac0901bd | not a remote branch head (merged or only in history; see the manifest rows above) |
| [DIG-339](issues/DIG-339.md) | pull_request | `` | `` | https://repo.digikal.com/jawad/digikal-knowledge-base/pulls/1 | — |
| [DIG-340](issues/DIG-340.md) | pull_request | `` | `` | https://repo.digikal.com/jawad/RustView/pulls/2 | — |
| [DIG-340](issues/DIG-340.md) | pull_request | `` | `` | https://repo.digikal.com/jawad/SkillOpt/pulls/2 | — |
| [DIG-340](issues/DIG-340.md) | pull_request | `` | `` | https://repo.digikal.com/jawad/adp/pulls/2 | — |
| [DIG-340](issues/DIG-340.md) | pull_request | `` | `` | https://repo.digikal.com/jawad/ai-engineering-governance/pulls/2 | — |
| [DIG-340](issues/DIG-340.md) | pull_request | `` | `` | https://repo.digikal.com/jawad/aletheia/pulls/2 | — |
| [DIG-340](issues/DIG-340.md) | pull_request | `` | `` | https://repo.digikal.com/jawad/browser-native-rts-poc/pulls/2 | — |
| [DIG-340](issues/DIG-340.md) | pull_request | `` | `` | https://repo.digikal.com/jawad/clear-technical-communication/pulls/2 | — |
| [DIG-340](issues/DIG-340.md) | pull_request | `` | `` | https://repo.digikal.com/jawad/honcho/pulls/28 | — |
| [DIG-340](issues/DIG-340.md) | pull_request | `` | `` | https://repo.digikal.com/jawad/humm/pulls/2 | — |
| [DIG-340](issues/DIG-340.md) | pull_request | `` | `` | https://repo.digikal.com/jawad/infrastructure/pulls/4 | — |
| [DIG-340](issues/DIG-340.md) | pull_request | `` | `` | https://repo.digikal.com/jawad/j-bot/pulls/2 | — |
| [DIG-340](issues/DIG-340.md) | pull_request | `` | `` | https://repo.digikal.com/jawad/jsync/pulls/2 | — |
| [DIG-340](issues/DIG-340.md) | pull_request | `` | `` | https://repo.digikal.com/jawad/n8n/pulls/2 | — |
| [DIG-340](issues/DIG-340.md) | pull_request | `` | `` | https://repo.digikal.com/jawad/nurikabe/pulls/88 | — |
| [DIG-340](issues/DIG-340.md) | pull_request | `` | `` | https://repo.digikal.com/jawad/production-ready-engineering-skill/pulls/2 | — |
| [DIG-340](issues/DIG-340.md) | pull_request | `` | `` | https://repo.digikal.com/jawad/sweeper/pulls/9 | — |
| [DIG-340](issues/DIG-340.md) | pull_request | `` | `` | https://repo.digikal.com/jawad/tvi/pulls/11 | — |
| [DIG-340](issues/DIG-340.md) | pull_request | `` | `` | https://repo.digikal.com/jawad/zimam/pulls/2 | — |

## Pull requests

| PR | State | Head branch | Head SHA | Base | Title |
|---|---|---|---|---|---|
| [#2](https://repo.digikal.com/jawad/humm/pulls/2) | merged | `paperclip/DIG-340` | `705ff5c4ba50377026c0149441ed51417d3b8018` | `main` | Link the Digikal knowledge base (DIG-340) |
| [#1](https://repo.digikal.com/jawad/humm/pulls/1) | open | `docs/paperclip-shutdown-2026-10-03` | `d7c60c287f0735240f772ca5a5ecf727ba49bcf1` | `main` | docs(paperclip): shutdown resume guide and task history archive (DIG-335) |

## Remote refs (7)

| Ref | SHA |
|---|---|
| `HEAD` | `582b4c1da7c123c81d9bc71210c2c0d5b89bec0c` |
| `refs/heads/docs/paperclip-shutdown-2026-10-03` | `d7c60c287f0735240f772ca5a5ecf727ba49bcf1` |
| `refs/heads/main` | `582b4c1da7c123c81d9bc71210c2c0d5b89bec0c` |
| `refs/heads/paperclip/DIG-340` | `705ff5c4ba50377026c0149441ed51417d3b8018` |
| `refs/pull/1/head` | `d7c60c287f0735240f772ca5a5ecf727ba49bcf1` |
| `refs/pull/2/head` | `705ff5c4ba50377026c0149441ed51417d3b8018` |
| `refs/tags/v1.0.0` | `dd54e609932f07b6b6bfcfdd21460393c1d9a1c9` |
