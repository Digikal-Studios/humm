# Cutoff inventory

Snapshot baseline export 2026-10-03T06:17Z (issues) / 06:24Z (git refs); Paperclip cutoff 2026-10-03T06:12:00Z ([DIG-332](issues/DIG-332.md#document-shutdown-cutoff)).

## Counts

| Measure | Primary issues | All issues in this archive |
|---|---|---|
| issues | 2 | 23 |
| comments | 2 | 130 |
| documents | 2 | 25 |
| document revisions | 2 | 42 |
| attachments | 0 | 2 |
| interactions | 0 | 11 |
| work products | 0 | 3 |
| deleted comments excluded | 0 | 0 |

Status at cutoff (primary): done 2.

Status at export (primary): done 2.

Attachments copied into this folder: 0 files, 0 bytes, verified against the Paperclip SHA-256 with `sha256sum -c data/attachments.sha256` run from this folder.

No attachment exceeds the large-file threshold.

## How issues were mapped here

- **Primary**: the issue's Paperclip project is linked to this repository.
- **Cross-reference copy**: the issue is a parent, blocker, blocked issue or textual reference of a primary issue, refers to one, or is a company-wide policy/shutdown record (DIG-250, DIG-261, DIG-299, DIG-305, DIG-327, DIG-332, DIG-333, DIG-334, DIG-335).

## Exclusions

- Secret values: none exported. Redactions applied in this archive: openai_key ×1.
- Deleted comments are excluded (count above).
- Paperclip runtime state is excluded: run ids, execution locks, adapter overrides, workspace settings, agent configuration and heartbeat logs. Agent configuration is restored from Paperclip itself, not from this archive.
- Cross-reference copies do not duplicate attachments or earlier document revisions; they link to the primary repository's copy.
- Links of the form `/DIG/...` that are not issue or attachment links (agent, run and approval pages) still point at Paperclip and work only while Paperclip runs. Agent mentions are kept as plain names.
- Relative links inside archived descriptions, comments and documents are kept verbatim. They resolve against the location the author wrote them for (usually the repository root or the company knowledge base), not against this folder.

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
| [DIG-299](issues/DIG-299.md) | in_progress | in_progress | QAEngineer | honcho Gate 4: prove run-owned Python/SDK targets before database contact |
| [DIG-305](issues/DIG-305.md) | blocked | blocked | DevGuru | honcho Gate 4: correct adjacent readiness notices and local quickstart |
| [DIG-327](issues/DIG-327.md) | in_review | in_review | CEO | CEO: missing-disposition sweep |
| [DIG-332](issues/DIG-332.md) | in_progress | in_progress | CEO | Paperclip shutdown preparation |
| [DIG-333](issues/DIG-333.md) | created after cutoff | in_progress | DevGuru | Shutdown: reconcile and finish already-started Honcho work |
| [DIG-334](issues/DIG-334.md) | created after cutoff | in_progress | ReleaseEngineer | Shutdown: preserve and verify source work across all repositories |
| [DIG-335](issues/DIG-335.md) | created after cutoff | in_progress | KnowledgeCurator | Shutdown: commit portable task history and resume guides to every repository |
