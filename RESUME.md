# Resuming humm after the Paperclip shutdown

**Posture: frozen.** Digikal stopped running this repository's work through Paperclip at the shutdown cutoff, 2026-10-03T06:12:00Z, and the archive authorises no new work. Every Paperclip issue for this repository was done at the export; none was left unfinished. Starting any item needs a new founder decision.

This page only points the way. The full record is the shutdown archive on this branch, and studio context lives in the knowledge base. Neither is copied here.

## 1. Open the archive

Start at [`docs/paperclip/shutdown-2026-10-03/README.md`](docs/paperclip/shutdown-2026-10-03/README.md) on `main`, then read, in order:

1. [`planned-work.md`](docs/paperclip/shutdown-2026-10-03/planned-work.md): what was unfinished, planned or shelved, and the freezes that apply.
2. [`refs.md`](docs/paperclip/shutdown-2026-10-03/refs.md): source rows, archive refs, pull requests and exact restore commands.
3. [`decisions.md`](docs/paperclip/shutdown-2026-10-03/decisions.md): founder and DevGuru decisions, blockers and review verdicts.
4. [`setup.md`](docs/paperclip/shutdown-2026-10-03/setup.md): setup and test commands quoted from this repository's docs.
- `docs/paperclip/shutdown-2026-10-03/issues/DIG-<n>.md`: one page per issue, with every plan revision, comment, review and attachment.

## 2. Recover source, WIP and plans

- **Source:** the default branch `main`. Every other branch and its head is listed in [`refs.md`](docs/paperclip/shutdown-2026-10-03/refs.md) with a restore command.
- **Plans and unfinished work:** [`planned-work.md`](docs/paperclip/shutdown-2026-10-03/planned-work.md) and the issue pages it links.
- **WIP and evidence:** this repository has no auxiliary archive branch; the default branch holds everything the archive needs.

Full offline check of the archive, with every auxiliary branch it needs checked out next to the clone:

```sh
git clone https://repo.digikal.com/jawad/humm.git humm   # a full clone fetches every branch listed below
cd humm
cd docs/paperclip/shutdown-2026-10-03
python3 tools/check_offline.py . --complete
sha256sum -c data/attachments.sha256   # the same hashes, checked by a second tool
```

`--complete` fails unless every hash manifest in the archive can be checked.

## 3. Company knowledge base (canonical)

Studio context (architecture, status, decisions and design across projects) lives only in the knowledge base. This page does not copy it; where the two disagree, this repository's own source wins.

```sh
git clone https://repo.digikal.com/jawad/digikal-knowledge-base.git
# open digikal-knowledge-base/knowledge/README.md, then knowledge/projects/humm/README.md
```
