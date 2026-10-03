# Blockers and decisions

## Founder decisions after the cutoff

- 2026-10-03T14:13:56Z: Honcho shelved; all Honcho product work cancelled, every state preserved ([honcho-shelving](issues/DIG-332.md#document-honcho-shelving), [founder comment](issues/DIG-332.md#comment-e20ee74e-7e11-4966-ac1e-d461721f7565)).
- 2026-10-03 08:57 UTC: everything needed to resume must be preserved in the repositories and work without Paperclip ([portable-preservation](issues/DIG-332.md#document-portable-preservation)).
- 2026-10-03 07:29 UTC: the canonical knowledge base becomes its own repository, linked from every project ([kb-repository-rollout](issues/DIG-332.md#document-kb-repository-rollout)).
- 2026-10-03 07:00 UTC: keep the onboarding repository token unchanged for now; values stay redacted ([DIG-332 credential-decision](issues/DIG-332.md#document-credential-decision)).

## Company-wide decisions that govern resumed work

- [DIG-250](issues/DIG-250.md): DevGuru adversarially reviews every deliverable before it is done.
- [DIG-261](issues/DIG-261.md): DevGuru holds the founder's delegated authority for technical decisions.
- [DIG-332](issues/DIG-332.md#document-shutdown-cutoff): shutdown cutoff of 2026-10-03T06:12:00Z; the nine todo/backlog tasks are founder-prohibited; started work was reconciled and preserved.
- DevGuru (DIG-261, technical): thirteen dormant Honcho plans were held under a coordination freeze ([DIG-333](issues/DIG-333.md#document-shutdown-disposition)), later superseded by the shelving.
- DevGuru (DIG-261, technical): honcho's archive pull request #26 stays unmerged because the Docker workflow on `main` publishes images; honcho's archive is read from branch `docs/paperclip-shutdown-2026-10-03`.

## Blocker graph for unfinished primary issues

No first-class blockers are recorded on unfinished primary issues.

## Decision and review documents

Review verdicts, plans and decision records stored as Paperclip documents on issues in this archive:

| Issue | Status at export | Document | Title | Revision |
|---|---|---|---|---|
| [DIG-1](issues/DIG-1.md#document-plan) | done | `plan` | Plan | 1 |
| [DIG-5](issues/DIG-5.md#document-design-page-input) | done | `design-page-input` | Design pages are written — do not stub them | 4 |
| [DIG-5](issues/DIG-5.md#document-seeding-inputs) | done | `seeding-inputs` | DIG-5 seeding inputs — the eighteen scan reports | 1 |
| [DIG-6](issues/DIG-6.md#document-domain-software-engineering) | done | `domain-software-engineering` | Publish-ready: knowledge/domains/software-engineering/README.md | 1 |
| [DIG-6](issues/DIG-6.md#document-domain-systems-architecture) | done | `domain-systems-architecture` | Publish-ready: knowledge/domains/systems-architecture/README.md | 1 |
| [DIG-6](issues/DIG-6.md#document-domain-tooling-pipeline) | done | `domain-tooling-pipeline` | Publish-ready: knowledge/domains/tooling-pipeline/README.md | 1 |
| [DIG-6](issues/DIG-6.md#document-review) | done | `review` | Six domain hubs — review request | 3 |
| [DIG-6](issues/DIG-6.md#document-ux-design-topic-page-owner) | done | `ux-design-topic-page-owner` | The ux-design topic page is DIG-38's write | 1 |
| [DIG-7](issues/DIG-7.md#document-design-page-sample) | done | `design-page-sample` | Design page sample (pointer to the filed sweeper page) | 7 |
| [DIG-7](issues/DIG-7.md#document-design-pages) | done | `design-pages` | Design pages for the knowledge base — accumulating handoff | 6 |
| [DIG-7](issues/DIG-7.md#document-domain-error-messages) | done | `domain-error-messages` | domains/ux-design — the four-part error message | 1 |
| [DIG-14](issues/DIG-14.md#document-report) | done | `report` | humm — Status Report | 1 |
| [DIG-26](issues/DIG-26.md#document-review) | done | `review` | humm — Design Review (UX / UI / interaction) | 1 |
| [DIG-39](issues/DIG-39.md#document-content) | done | `content` | Software Engineering — domain page | 1 |
| [DIG-40](issues/DIG-40.md#document-content) | done | `content` | Systems Architecture — domain page | 1 |
| [DIG-41](issues/DIG-41.md#document-content) | done | `content` | Tooling and Pipeline — domain page | 1 |
| [DIG-70](issues/DIG-70.md#document-content) | done | `content` | knowledge/domains/ux-design/README.md | 1 |
| [DIG-76](issues/DIG-76.md#document-decisions) | done | `decisions` | Project decision logs — eight projects | 1 |
| [DIG-250](issues/DIG-250.md#document-review-policy) | done | `review-policy` | Adversarial review policy for Digikal agents | 1 |
| [DIG-261](issues/DIG-261.md#document-decision-policy) | done | `decision-policy` | Delegated board decisions (DIG-261) | 1 |
| [DIG-299](issues/DIG-299.md#document-ceo-escalation) | cancelled | `ceo-escalation` | CEO escalation disposition — changes requested after round 3 | 1 |
| [DIG-299](issues/DIG-299.md#document-plan) | cancelled | `plan` | Gate 4 execution plan and handoff | 1 |
| [DIG-299](issues/DIG-299.md#document-review) | cancelled | `review` | DevGuru review: reject a3bcce0 (round 3; CEO escalation) | 3 |
| [DIG-299](issues/DIG-299.md#document-rework-routing) | cancelled | `rework-routing` | CEO-directed design, implementation and review handoff | 2 |
| [DIG-299](issues/DIG-299.md#document-validation) | cancelled | `validation` | Validation and author handoff, round 3 (a3bcce0) | 3 |
| [DIG-305](issues/DIG-305.md#document-merge-receipt) | done | `merge-receipt` | merge-receipt | 1 |
| [DIG-305](issues/DIG-305.md#document-plan) | done | `plan` | Plan | 1 |
| [DIG-305](issues/DIG-305.md#document-review) | done | `review` | Final review — approved at b1555b7 | 3 |
| [DIG-305](issues/DIG-305.md#document-validation) | done | `validation` | validation | 3 |
| [DIG-327](issues/DIG-327.md#document-recovery-dig-306) | blocked | `recovery-dig-306` | DIG-306: confirmed execution hold and evidence gap | 4 |
| [DIG-327](issues/DIG-327.md#document-sweep) | blocked | `sweep` | Recovery verified: two resumed, one hold remains | 5 |
| [DIG-332](issues/DIG-332.md#document-credential-decision) | blocked | `credential-decision` | Founder decision: keep onboarding token unchanged for now | 2 |
| [DIG-332](issues/DIG-332.md#document-credential-rotation) | blocked | `credential-rotation` | Guided revocation: old jawad onboarding token | 2 |
| [DIG-332](issues/DIG-332.md#document-honcho-shelving) | blocked | `honcho-shelving` | Honcho shelving decision, cancellation inventory and preservation requirements | 3 |
| [DIG-332](issues/DIG-332.md#document-kb-repository-rollout) | blocked | `kb-repository-rollout` | Canonical KB repository migration and project adoption | 1 |
| [DIG-332](issues/DIG-332.md#document-knowledge-storage) | blocked | `knowledge-storage` | KnowledgeCurator: storage locations and verified repository copy | 2 |
| [DIG-332](issues/DIG-332.md#document-portable-preservation) | blocked | `portable-preservation` | Required: complete repository handover without Paperclip | 1 |
| [DIG-332](issues/DIG-332.md#document-shutdown-control-state) | blocked | `shutdown-control-state` | Shutdown control state and continuation paths | 2 |
| [DIG-332](issues/DIG-332.md#document-shutdown-cutoff) | blocked | `shutdown-cutoff` | Shutdown cutoff and acceptance record | 5 |
| [DIG-332](issues/DIG-332.md#document-shutdown-progress) | blocked | `shutdown-progress` | Shutdown progress and credential decision | 4 |
| [DIG-333](issues/DIG-333.md#document-archive-handoff) | done | `archive-handoff` | Honcho archive handoff without publication | 1 |
| [DIG-333](issues/DIG-333.md#document-ceo-review) | done | `ceo-review` | CEO review: Honcho shelving disposition approved | 3 |
| [DIG-333](issues/DIG-333.md#document-completion-boundary) | done | `completion-boundary` | Shutdown completion boundaries and source reuse | 5 |
| [DIG-333](issues/DIG-333.md#document-decisions) | done | `decisions` | Shutdown decisions and applied receipts | 10 |
| [DIG-333](issues/DIG-333.md#document-shelving-disposition) | done | `shelving-disposition` | Honcho shelving: cancellation, PR posture and preservation inputs | 5 |
| [DIG-333](issues/DIG-333.md#document-shelving-verification) | done | `shelving-verification` | Final independent Honcho shelving verification | 1 |
| [DIG-333](issues/DIG-333.md#document-shutdown-disposition) | done | `shutdown-disposition` | Shutdown disposition and completion boundaries | 9 |
| [DIG-334](issues/DIG-334.md#document-ceo-disposition) | done | `ceo-disposition` | CEO disposition — third-review escalation | 1 |
| [DIG-334](issues/DIG-334.md#document-decisions) | done | `decisions` | Final source delta dependency decision | 3 |
| [DIG-334](issues/DIG-334.md#document-review) | done | `review` | DevGuru adversarial review — approved baseline | 5 |
| [DIG-334](issues/DIG-334.md#document-source-preservation) | done | `source-preservation` | Source preservation manifest — shutdown 2026-10-03 (rev 4) | 5 |
| [DIG-335](issues/DIG-335.md#document-preservation-index) | in_progress | `preservation-index` | Preservation index | 5 |
| [DIG-335](issues/DIG-335.md#document-review) | in_progress | `review` | DevGuru adversarial review — round 1 (required changes) | 1 |
| [DIG-338](issues/DIG-338.md#document-review) | done | `review` | DevGuru adversarial review — approved final preservation delta | 3 |
| [DIG-338](issues/DIG-338.md#document-review-brief) | done | `review-brief` | Review brief — DIG-338 final delta (round 2) | 3 |
| [DIG-338](issues/DIG-338.md#document-source-preservation) | done | `source-preservation` | Source preservation — final delta (DIG-338) | 3 |
| [DIG-339](issues/DIG-339.md#document-migration) | done | `migration` | DIG-339 migration record | 9 |
| [DIG-339](issues/DIG-339.md#document-plan) | done | `plan` | KB migration and Paperclip-independent evidence correction | 2 |
| [DIG-339](issues/DIG-339.md#document-review) | done | `review` | DIG-339 adversarial review — portability round 2 | 6 |
| [DIG-340](issues/DIG-340.md#document-plan) | done | `plan` | Plan | 2 |
| [DIG-340](issues/DIG-340.md#document-project-adoption) | done | `project-adoption` | Project adoption matrix | 5 |
| [DIG-340](issues/DIG-340.md#document-review) | done | `review` | DevGuru review — final portable adoption approval | 3 |

## Board interactions

| Issue | Kind | Status | Created |
|---|---|---|---|
| [DIG-1](issues/DIG-1.md) | ask_user_questions | answered | 2026-09-25T10:37:12.484Z |
| [DIG-1](issues/DIG-1.md) | ask_user_questions | answered | 2026-09-25T11:05:55.959Z |
| [DIG-1](issues/DIG-1.md) | request_checkbox_confirmation | accepted | 2026-09-25T11:14:54.543Z |
| [DIG-4](issues/DIG-4.md) | request_confirmation | accepted | 2026-09-25T14:12:56.279Z |
| [DIG-5](issues/DIG-5.md) | request_confirmation | accepted | 2026-09-25T18:33:02.814Z |
| [DIG-6](issues/DIG-6.md) | request_confirmation | cancelled | 2026-09-25T18:36:46.250Z |
| [DIG-6](issues/DIG-6.md) | request_confirmation | cancelled | 2026-09-26T03:38:31.842Z |
| [DIG-6](issues/DIG-6.md) | request_confirmation | accepted | 2026-09-26T03:39:01.018Z |
| [DIG-7](issues/DIG-7.md) | ask_user_questions | answered | 2026-09-25T16:09:06.121Z |
| [DIG-305](issues/DIG-305.md) | request_confirmation | accepted | 2026-10-03T09:07:05.231Z |
| [DIG-327](issues/DIG-327.md) | request_confirmation | expired | 2026-10-01T10:01:05.937Z |
| [DIG-327](issues/DIG-327.md) | request_confirmation | expired | 2026-10-01T10:17:10.810Z |
| [DIG-327](issues/DIG-327.md) | request_confirmation | rejected | 2026-10-03T06:20:37.473Z |
| [DIG-327](issues/DIG-327.md) | ask_user_questions | answered | 2026-10-03T07:33:04.415Z |
| [DIG-327](issues/DIG-327.md) | ask_user_questions | answered | 2026-10-03T07:56:01.228Z |
| [DIG-327](issues/DIG-327.md) | ask_user_questions | answered | 2026-10-03T07:57:51.269Z |
| [DIG-327](issues/DIG-327.md) | ask_user_questions | cancelled | 2026-10-03T08:05:48.975Z |
| [DIG-332](issues/DIG-332.md) | ask_user_questions | answered | 2026-10-03T06:41:17.489Z |
| [DIG-332](issues/DIG-332.md) | ask_user_questions | answered | 2026-10-03T06:53:43.825Z |
| [DIG-338](issues/DIG-338.md) | request_confirmation | accepted | 2026-10-03T09:01:57.542Z |
| [DIG-339](issues/DIG-339.md) | ask_user_questions | answered | 2026-10-03T07:44:27.930Z |
| [DIG-339](issues/DIG-339.md) | request_confirmation | accepted | 2026-10-03T10:37:05.155Z |
