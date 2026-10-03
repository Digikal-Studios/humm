# Cutoff inventory

Snapshot closing delta 2026-10-03: Paperclip records exported from 2026-10-03T17:39:09Z, Git refs read at 2026-10-03T17:41:58Z. Original Paperclip cutoff 2026-10-03T06:12:00Z ([DIG-332](issues/DIG-332.md#document-shutdown-cutoff)); every shutdown-era record created after it, up to the export, is included.

## Counts

| Measure | Primary issues | All issues in this archive |
|---|---|---|
| issues | 2 | 27 |
| comments | 2 | 375 |
| documents | 2 | 62 |
| document revisions | 2 | 175 |
| attachments | 0 | 15 |
| interactions | 0 | 22 |
| work products | 0 | 73 |
| deleted comments excluded | 0 | 0 |

Status at cutoff (primary): done 2.

Status at export (primary): done 2.

No attachment belongs to this repository's issues, so `data/attachments.sha256` is empty.

No attachment exceeds the large-file threshold.

## How issues were mapped here

- **Primary**: the issue's Paperclip project is linked to this repository.
- **Cross-reference copy**: the issue is a parent, blocker, blocked issue or textual reference of a primary issue, refers to one, or is a company-wide policy/shutdown record (DIG-250, DIG-261, DIG-299, DIG-305, DIG-327, DIG-332, DIG-333, DIG-334, DIG-335, DIG-337, DIG-338, DIG-339, DIG-340).

<a id="exclusions"></a>

## Exclusions and redactions

- Secret values: none exported. Redactions applied in this archive: assigned_secret ×1, openai_key ×1. The repository token pasted into a DIG-2 answer stays redacted under the founder's decision to keep it unchanged for now (infrastructure `credential-decision-2026-10-03.md`).
- Deleted comments are excluded (count above).
- Agent runs: every run recorded against an issue is listed on that issue's page (4 for primary issues here). Raw run transcripts are excluded as runtime output whose results are already recorded as comments, documents, attachments and commits, and which carry host environment detail. The exception is the shelved Honcho work that stopped mid-run (DIG-343, DIG-344 and post-cutoff DIG-299 runs): 25 transcripts are kept, redacted, on branch `archive/paperclip-shutdown-2026-10-03-large-attachments` of the issue's primary repository; 2 runs in that set never wrote a log on the host.
- Paperclip runtime state is excluded: execution locks, leases, adapter overrides, workspace runtime settings, agent memory and sessions. Agent role instructions are archived once, in the infrastructure archive under `agents/`.
- Cross-reference copies do not duplicate attachments or earlier document revisions; they link to the primary repository's copy on its docs branch.
- Links: Paperclip issue, document, comment, interaction, attachment, run and approval links are rewritten to the archived record (a file in this folder or in another repository's docs branch); agent links go to the archived role instructions. Same-record anchors such as `(#document-review)` resolve to the issue page that holds them. Authored relative links resolve in this repository, the source issue's repository or, when exactly one file matches, the knowledge base. Every link that cannot be resolved is shown as text with its original target in code and is listed with its reason in [data/unresolved-links.tsv](data/unresolved-links.tsv) (reasons: `paperclip-ui-page`: a Paperclip screen (agent dashboard, inbox, settings) with no archived record; `target-absent-in-source`: the cited issue, document, comment, attachment, run or approval does not exist in the Paperclip export; `placeholder`: a template or example target, not a real record; `ambiguous-relative-path`: an authored relative path that matches more than one file, so no single target is chosen; `unknown-relative-path`: an authored relative path that matches no file in this repository, the source issue's repository or the knowledge base; `fragment-without-context`: a same-page anchor whose source record is unknown at this point of the page). Counts for this folder: agent link to archived role instructions 7, agent mention kept as name 27, attachment link 23, authored relative link resolved in knowledge base 20, authored relative link resolved in source repository 18, issue link 1602, issue link with anchor 782, run link 1, same-issue anchor 1.
- Source code, task branches, stashes and uncommitted work are not in this folder; they are on Git refs listed in [refs.md](refs.md) from ReleaseEngineer's manifest.



## Primary issues

| Issue | Status at cutoff | Status at export | Owner | Title |
|---|---|---|---|---|
| [DIG-14](issues/DIG-14.md) | done | done | TechLead | Scan and report: humm |
| [DIG-26](issues/DIG-26.md) | done | done | GameExperienceDesigner | Design review: humm |

## Cross-reference copies

| Issue | Status at cutoff | Status at export | Owner | Title |
|---|---|---|---|---|
| [DIG-1](issues/DIG-1.md) | done | done | CEO | Paperclip onboarding |
| [DIG-4](issues/DIG-4.md) | done | done | CEO | Initial scan and status report per project |
| [DIG-5](issues/DIG-5.md) | done | done | CEO | Seed knowledge base project pages from scan reports |
| [DIG-6](issues/DIG-6.md) | done | done | CEO | Seed knowledge base domain foundations |
| [DIG-7](issues/DIG-7.md) | done | done | CEO | Design-side review of existing projects |
| [DIG-33](issues/DIG-33.md) | done | done | GameExperienceDesigner | Build remaining design pages and hand off to Knowledge Curator |
| [DIG-39](issues/DIG-39.md) | done | done | TechLead | Domain page: Software Engineering |
| [DIG-40](issues/DIG-40.md) | done | done | TechLead | Domain page: Systems Architecture |
| [DIG-41](issues/DIG-41.md) | done | done | TechLead | Domain page: Tooling and Pipeline |
| [DIG-42](issues/DIG-42.md) | done | done | KnowledgeCurator | Reconcile §9.6 with the twelve landed design pages — Source column and ID format |
| [DIG-70](issues/DIG-70.md) | done | done | GameExperienceDesigner | Domain page: UX Design |
| [DIG-76](issues/DIG-76.md) | done | done | TechLead | Project decision logs: what binding decisions does each project carry? |
| [DIG-250](issues/DIG-250.md) | done | done | CEO | Assign adversarial reviews to DevGuru |
| [DIG-261](issues/DIG-261.md) | done | done | CEO | Resolve blockers |
| [DIG-299](issues/DIG-299.md) | in_progress | cancelled | QAEngineer | honcho Gate 4: prove run-owned Python/SDK targets before database contact |
| [DIG-305](issues/DIG-305.md) | blocked | done | TechLead | honcho Gate 4: correct adjacent readiness notices and local quickstart |
| [DIG-327](issues/DIG-327.md) | in_review | blocked | CEO | CEO: missing-disposition sweep |
| [DIG-332](issues/DIG-332.md) | in_progress | blocked | CEO | Paperclip shutdown preparation |
| [DIG-333](issues/DIG-333.md) | created after cutoff | done | CEO | Shutdown: record Honcho shelving and preservation disposition |
| [DIG-334](issues/DIG-334.md) | created after cutoff | done | ReleaseEngineer | Shutdown: preserve and verify source work across all repositories |
| [DIG-335](issues/DIG-335.md) | created after cutoff | in_progress | KnowledgeCurator | Shutdown: commit portable task history and resume guides to every repository |
| [DIG-337](issues/DIG-337.md) | created after cutoff | done | KnowledgeCurator | Shutdown archive: final delta after source preservation and in-flight disposition (DIG-335 continuation) |
| [DIG-338](issues/DIG-338.md) | created after cutoff | done | ReleaseEngineer | Shutdown: final source-preservation delta before documentation export |
| [DIG-339](issues/DIG-339.md) | created after cutoff | done | KnowledgeCurator | Populate the canonical digikal-knowledge-base repository |
| [DIG-340](issues/DIG-340.md) | created after cutoff | done | unassigned | Link all 18 project repositories to the canonical knowledge base |
