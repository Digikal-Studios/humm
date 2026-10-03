# DIG-1: Paperclip onboarding

> Cross-reference copy. The primary archive for DIG-1 is in the `infrastructure` repository: https://repo.digikal.com/jawad/infrastructure/src/branch/docs/paperclip-shutdown-2026-10-03/docs/paperclip/shutdown-2026-10-03/issues/DIG-1.md. Attachments and earlier document revisions are stored there.

| Field | Value |
|---|---|
| Status at cutoff (2026-10-03T06:12:00Z) | done |
| Status at export (2026-10-03 final delta (records exported from 2026-10-03T16:42:39Z)) | done |
| Priority | medium |
| Owner (assignee) | CEO |
| Created by | Board user (founder) |
| Paperclip project | Onboarding |
| Primary repository archive | infrastructure |
| Parent | — |
| Created / updated | 2026-09-25T10:37:12.315Z / 2026-09-25T11:35:11.270Z |
| Completed / cancelled | 2026-09-25T11:35:11.193Z / — |
| Blocked by | — |
| Blocks | — |
| Paperclip id | `2cefd367-657a-4a47-925a-a03c05b175c6` |

## Description

This is the user's first task in Paperclip. Your job is to understand what they want and propose a path forward. A greeting and an opening question card were already posted for you; the card offered two choices: "Interview me and propose a plan and an agent team to execute it." (option `interview`) or "I have a task in mind" (option `task`, with a text field). You are running because the user answered that card (the answer is in your wake payload) or wrote a message instead of answering. Don't re-introduce yourself and don't post the opening card again.

Work in this order.

1. Take the path the user picked.

   - `interview` → reply with ONE ask_user_questions card of 3–4 questions that pin down what the organization does, what they want to achieve first, any constraints (time, budget, tools), and what "done" looks like. Don't guess; ask. Don't post anything else before the card. The answers lead to the plan-and-team path in step 2.

   - `task` → the text they typed is the task. If it is clear enough to propose on, go straight to step 2. If not, reply with ONE ask_user_questions card of 2–3 questions specific to their message (concrete goal, constraints, what "done" looks like), then go to step 2.

   - If they wrote a message instead of answering the card, treat the message as the `task` path.

2. Propose, don't decide. From what you now know, pick the path:

   - They want a plan and/or a team → write a short `plan` document (goal, approach, team as one line per hire: name, role, responsibility; follow-up tasks). Then post ONE request_checkbox_confirmation targeting the plan, each hire and follow-up task as its own option, checked by default, each with a stable id. Keep the card's message to a line or two and point to the Plan in the right sidebar.

   - They want one thing done now → post ONE request_confirmation that says, in a few lines, what you will do and what they will get (and by when, if you can say). No plan document.

3. Wait. Do nothing until a card is accepted. If they ask for changes, revise and re-confirm. Hiring or creating tasks without an accepted card is never allowed on this task.

4. On acceptance, execute only what was approved: hire the checked agents, create and delegate the checked tasks, or do the single task yourself and post the result as a document on this task.

## Documents (latest revision)

<a id="document-plan"></a>

### Document `plan`: Plan

Revision 1 (id `414554df-5e27-46bd-86c5-4ebf25435166`), updated 2026-09-25T11:14:35.957Z by CEO.

#### Plan: Digikal project discovery, knowledge base, and expert team

##### Goal

Give Digikal a clear picture of every project in flight (done, planned, mid-implementation), a well-structured knowledge base that serves current and future projects, and a small expert team that grows as the projects reveal what expertise is needed.

##### Constraints

- **Budget.** Start with three agents, not ten. Each hire has a monthly budget cap. Specialists (engine-specific, backend, audio, etc.) are proposed only after the scan shows which stacks actually exist.
- **No repositories are connected yet.** The Onboarding project has an empty managed folder. The scan cannot start until you link your repos or local working directories to Paperclip projects. Task 1 below walks you through that.

##### Approach

1. **Connect the code.** You link each repo or working directory as a Paperclip project. I create one project per codebase so scans and reports stay separate.
2. **Scan and report.** The Tech Lead reads each codebase and produces a status report per project: what it is, stack, what is built, what is in progress, what is planned (from TODOs, issues, roadmaps, branches), risks, and open questions for you.
3. **Design the knowledge base before filling it.** The Knowledge Curator proposes the structure, naming conventions, and maintenance rules first, so the base does not rot. Proposed shape: a `knowledge/` tree with `projects/<name>/` (overview, architecture, status, decisions, glossary) and `domains/` (software engineering, systems architecture, game design, UI, UX, tooling), each page with an owner, last-reviewed date, and a "confidence" tag. Reviewed by you before content lands.
4. **Seed the base.** Project pages come from the scan reports. Domain pages start from a short curated set of foundations, written to Digikal's context rather than generic textbook content.
5. **Grow the team from evidence.** After the scan, I propose specialist hires matched to the stacks and gaps found (for example a Unity or Godot expert, a backend lead, an audio designer). Each goes through a confirmation card and company approval.

##### Team (initial hires)

- **Tech Lead** — role: engineering — Scans codebases, writes per-project status reports, owns architecture and engineering pages in the knowledge base, reviews technical work from later hires.
- **Knowledge Curator** — role: general — Designs and maintains the knowledge base structure, enforces conventions, turns scan reports and domain notes into well-linked pages, runs periodic freshness reviews.
- **Game & Experience Designer** — role: general — Covers game design, UI, and UX. Reviews existing projects from a player and user perspective, writes the design-side project pages and the game design / UI / UX domain foundations. Splits into separate roles later if workload justifies it.

All three run on the same Claude adapter as me, with managed instructions and a monthly budget cap you set at approval time. No timer heartbeats; they work only when a task wakes them.

##### Follow-up tasks

1. **Connect Digikal repositories and working directories** — CEO. I post a short guide and a question card listing what to link. Blocks everything below.
2. **Initial scan and status report per project** — Tech Lead. One child task per connected codebase. Output: a status document per project on its task.
3. **Knowledge base structure and maintenance rules** — Knowledge Curator. Output: a design document for your review, then the empty skeleton.
4. **Seed project pages from scan reports** — Knowledge Curator with Tech Lead. Blocked by 2 and 3.
5. **Seed domain foundations** — Knowledge Curator with Game & Experience Designer and Tech Lead. Software engineering, systems architecture, game design, UI, UX. Blocked by 3.
6. **Design-side review of existing projects** — Game & Experience Designer. Player and user perspective on each scanned project. Blocked by 2.
7. **Team expansion proposal** — CEO. After the scan, a card proposing specialists matched to what was found. Blocked by 2.

##### What "done" looks like for this phase

- Roster of agents with their responsibilities and budgets visible in Paperclip.
- One status document per project answering: built, in progress, planned.
- Knowledge base skeleton in place with rules for adding and reviewing pages, and initial project and domain pages filled.
- A concrete specialist hiring proposal based on the scan.


## Issue-thread interactions

<a id="interaction-0f67f48d-9498-413f-ba22-99b98c0fd776"></a>

### ask_user_questions — answered — 2026-09-25T10:37:12.484Z

From CEO; resolved 2026-09-25T10:39:08.245Z by Board user (founder).

```json
{
 "title": null,
 "summary": null,
 "payload": {
  "version": 1,
  "submitLabel": "Continue",
  "supersedeOnUserComment": true,
  "questions": [
   {
    "id": "first-task-opening",
    "prompt": "What would you like to do?",
    "helpText": null,
    "selectionMode": "single",
    "required": true,
    "options": [
     {
      "id": "interview",
      "label": "Interview me and propose a plan and an agent team to execute it.",
      "description": "A few questions about what you're building, then a short plan and the team to carry it out, for you to approve."
     },
     {
      "id": "task",
      "label": "I have a task in mind",
      "description": "Describe it and I'll propose how to get it done.",
      "freeText": true
     }
    ]
   }
  ]
 },
 "result": {
  "version": 1,
  "answers": [
   {
    "questionId": "first-task-opening",
    "optionIds": [
     "interview"
    ]
   }
  ],
  "summaryMarkdown": null
 }
}
```

<a id="interaction-657582fd-d059-4ae5-95a3-0c95d77cb66e"></a>

### ask_user_questions — answered — 2026-09-25T11:05:55.959Z

From CEO; resolved 2026-09-25T11:13:23.899Z by Board user (founder).

```json
{
 "title": "Tell me about Digikal",
 "summary": null,
 "payload": {
  "version": 1,
  "supersedeOnUserComment": true,
  "questions": [
   {
    "id": "org",
    "prompt": "What does Digikal do, and who are your customers or users?",
    "selectionMode": "single",
    "required": true,
    "options": [
     {
      "id": "describe-org",
      "label": "I'll describe it",
      "freeText": true
     }
    ]
   },
   {
    "id": "first-goal",
    "prompt": "What is the first thing you want this team to achieve? Be as concrete as you can.",
    "selectionMode": "single",
    "required": true,
    "options": [
     {
      "id": "describe-goal",
      "label": "I'll describe it",
      "freeText": true
     }
    ]
   },
   {
    "id": "constraints",
    "prompt": "Any constraints I should plan around? Pick all that apply and add details.",
    "selectionMode": "multi",
    "required": false,
    "allowOther": true,
    "options": [
     {
      "id": "deadline",
      "label": "Deadline",
      "description": "There is a date this needs to be done by."
     },
     {
      "id": "budget",
      "label": "Budget",
      "description": "Limited spend on agents or tools."
     },
     {
      "id": "tools",
      "label": "Specific tools or repos",
      "description": "Must use or avoid particular systems."
     },
     {
      "id": "none",
      "label": "No hard constraints yet"
     }
    ]
   },
   {
    "id": "done",
    "prompt": "What does \"done\" look like for that first goal? What would you check to know it worked?",
    "selectionMode": "single",
    "required": true,
    "options": [
     {
      "id": "describe-done",
      "label": "I'll describe it",
      "freeText": true
     }
    ]
   }
  ]
 },
 "result": {
  "version": 1,
  "answers": [
   {
    "questionId": "org",
    "optionIds": [],
    "otherText": "Digikal is a software development studio. We develop all sort of applications, with a particular focus on game development. Our customers vary based on project type and scope.&#x20;"
   },
   {
    "questionId": "first-goal",
    "optionIds": [],
    "otherText": "Understand the existing projects we are working on at digikal, know what has been done so far, what is currently planned for implementation, and what is currently mid-implementation. I also need them to build a knowledge base that we can use for these projects, as well as future projects. This knowledge base should contain project information, as well as general knowledge relevant to software development, software engineering, systems architecture, game design, UI design, UX design, etc. It must be well structured and designed to be easy to maintain and grow over time. I also want the team to have experts in all relevant fields, starting with the projects that are already built (or being built at the moment)."
   },
   {
    "questionId": "constraints",
    "optionIds": [
     "budget"
    ]
   },
   {
    "questionId": "done",
    "optionIds": [],
    "otherText": "I would check the roster of team members, their plan for what needs to be done, their agents/harnesses etc., the outcome of the initial scan of my repo and working directories, etc.&#x20;"
   }
  ],
  "summaryMarkdown": null
 }
}
```

<a id="interaction-b26446ad-7ff1-432b-8639-7191c6fe09c3"></a>

### request_checkbox_confirmation — accepted — 2026-09-25T11:14:54.543Z

From CEO; resolved 2026-09-25T11:30:38.751Z by Board user (founder).

```json
{
 "title": "Approve plan, team, and first tasks",
 "summary": "Uncheck anything you don't want yet, then accept.",
 "payload": {
  "version": 1,
  "prompt": "The full Plan is in the right sidebar. Check what I should go ahead with.",
  "detailsMarkdown": "Three hires to start (budget-conscious), plus the follow-up tasks. Specialist hires come later, based on what the scan finds. Note: no repos are connected yet, so task 1 gates the scan.",
  "options": [
   {
    "id": "plan",
    "label": "Plan: discovery, knowledge base, expert team",
    "description": "Approve the overall approach in the Plan document."
   },
   {
    "id": "hire-tech-lead",
    "label": "Hire: Tech Lead (engineering)",
    "description": "Scans codebases, writes per-project status reports, owns engineering and architecture knowledge."
   },
   {
    "id": "hire-knowledge-curator",
    "label": "Hire: Knowledge Curator (general)",
    "description": "Designs and maintains the knowledge base structure and conventions."
   },
   {
    "id": "hire-game-experience-designer",
    "label": "Hire: Game & Experience Designer (general)",
    "description": "Game design, UI, and UX. Reviews projects from a player/user view and writes design domain pages."
   },
   {
    "id": "task-connect-repos",
    "label": "Task 1: Connect repositories and working directories (CEO)",
    "description": "Guide and question card for linking your code to Paperclip. Gates the scan."
   },
   {
    "id": "task-initial-scan",
    "label": "Task 2: Initial scan and status report per project (Tech Lead)",
    "description": "Built / in progress / planned, per codebase."
   },
   {
    "id": "task-kb-structure",
    "label": "Task 3: Knowledge base structure and maintenance rules (Curator)",
    "description": "Design doc for your review, then the skeleton."
   },
   {
    "id": "task-kb-projects",
    "label": "Task 4: Seed project pages from scan reports (Curator + Tech Lead)"
   },
   {
    "id": "task-kb-domains",
    "label": "Task 5: Seed domain foundations (Curator + Designer + Tech Lead)",
    "description": "Software engineering, architecture, game design, UI, UX."
   },
   {
    "id": "task-design-review",
    "label": "Task 6: Design-side review of existing projects (Designer)"
   },
   {
    "id": "task-team-expansion",
    "label": "Task 7: Team expansion proposal after scan (CEO)",
    "description": "Specialist hires matched to the stacks found."
   }
  ],
  "defaultSelectedOptionIds": [
   "plan",
   "hire-tech-lead",
   "hire-knowledge-curator",
   "hire-game-experience-designer",
   "task-connect-repos",
   "task-initial-scan",
   "task-kb-structure",
   "task-kb-projects",
   "task-kb-domains",
   "task-design-review",
   "task-team-expansion"
  ],
  "minSelected": 0,
  "acceptLabel": "Approve selected",
  "rejectLabel": "Request changes",
  "rejectRequiresReason": true,
  "rejectReasonLabel": "What should change?",
  "allowDeclineReason": true,
  "supersedeOnUserComment": true,
  "target": {
   "type": "issue_document",
   "issueId": "2cefd367-657a-4a47-925a-a03c05b175c6",
   "documentId": "af454204-48e5-4f08-a09b-d8d55ac74fbe",
   "key": "plan",
   "revisionId": "414554df-5e27-46bd-86c5-4ebf25435166",
   "revisionNumber": 1
  }
 },
 "result": {
  "version": 1,
  "outcome": "accepted",
  "selectedOptionIds": [
   "plan",
   "hire-tech-lead",
   "hire-knowledge-curator",
   "hire-game-experience-designer",
   "task-connect-repos",
   "task-initial-scan",
   "task-kb-structure",
   "task-kb-projects",
   "task-kb-domains",
   "task-design-review",
   "task-team-expansion"
  ]
 }
}
```

## Comments (7)

<a id="comment-fe924ace-426a-48eb-b6c4-39cb65e85d4e"></a>

### 2026-09-25T10:37:12.440Z — CEO

Comment id `fe924ace-426a-48eb-b6c4-39cb65e85d4e`.

Welcome to Paperclip! I'm CEO, your first agent teammate. Pick how you'd like to start and I'll take it from there.

<a id="comment-39b2edc8-bfb0-43e3-893e-af267966da86"></a>

### 2026-09-25T10:39:12.979Z — CEO

Comment id `39b2edc8-bfb0-43e3-893e-af267966da86`; written by run `e4fb6029-6915-4174-98eb-c649ab510270`.

ACP agent reported a terminal access failure.

<a id="comment-dc15f6e3-46c5-40a9-8b48-c3eb5ece3c54"></a>

### 2026-09-25T10:40:10.502Z — CEO

Comment id `dc15f6e3-46c5-40a9-8b48-c3eb5ece3c54`; written by run `86152ab2-5c4b-4dca-bc80-54b16477f0e3`.

ACP agent reported a terminal access failure.

<a id="comment-a1e87af2-29a9-46a2-bbfa-06e9bb152ef2"></a>

### 2026-09-25T10:41:06.038Z — CEO

Comment id `a1e87af2-29a9-46a2-bbfa-06e9bb152ef2`; written by run `7b6affa5-3219-4d9c-9745-4438527e2419`.

ACP agent reported a terminal access failure.

<a id="comment-36a97ec6-9c0c-414b-b3c5-de62c47c3923"></a>

### 2026-09-25T11:06:00.912Z — CEO

Comment id `36a97ec6-9c0c-414b-b3c5-de62c47c3923`; written by run `5ce5ff19-fa8a-477b-929b-ce4f97518b16`.

Waiting for your answers in the interview card above. Once you reply, I will draft a plan and propose a team for your approval.

<a id="comment-8cf824bd-833f-45f8-83f4-3cd036f4778d"></a>

### 2026-09-25T11:14:59.304Z — CEO

Comment id `8cf824bd-833f-45f8-83f4-3cd036f4778d`; written by run `dbef75a0-848b-4d0d-9d50-f141438e310a`.

Plan is in the right sidebar. Approve it (or uncheck items) in the card above and I will hire the checked agents and create the checked tasks. Nothing is hired or created until you accept.

<a id="comment-d8cec90d-544d-48ef-a744-5d0006842593"></a>

### 2026-09-25T11:35:11.257Z — CEO

Comment id `d8cec90d-544d-48ef-a744-5d0006842593`; written by run `49bb092e-91c3-496d-b89b-418f8156efc9`.

##### Plan approved and executed

**Hired (all active, reporting to CEO, $50/month cap each, no timer heartbeats):**
- TechLead (historical link `/DIG/agents/c76ac668-7eaa-4eab-84ee-de25fd9242f5`) — engineer. Codebase scans, status reports, engineering and architecture knowledge. Instruction source: generic baseline guide with a focused engineering lens set (Coder template was too implementation-centric for a read-only discovery role).
- KnowledgeCurator (historical link `/DIG/agents/45dcbfc4-d7cc-4b67-ac88-f1ce7a9c83f0`) — general. Knowledge base structure, conventions, curation, freshness. Instruction source: generic baseline guide.
- GameExperienceDesigner (historical link `/DIG/agents/d511c851-1685-47e9-818b-73e49bd651fe`) — general. Game design, UI, UX reviews and knowledge. Instruction source: adapted from the UX Designer template: charter rewritten for games, game-design lens group added, design-system and visual-truth sections replaced with a review output bar.

No permission-expanding skills or adapter settings were granted. All three are read-only on repositories by instruction.

**Tasks created:**
- DIG-2 Connect repositories and working directories — CEO, in review, waiting on your answer.
- DIG-3 Knowledge base structure and maintenance rules — KnowledgeCurator, already in progress.
- DIG-4 Initial scan and status report per project — TechLead, blocked by DIG-2.
- DIG-5 Seed project pages — KnowledgeCurator, blocked by DIG-3 and DIG-4.
- DIG-6 Seed domain foundations — KnowledgeCurator, blocked by DIG-3.
- DIG-7 Design-side review — GameExperienceDesigner, blocked by DIG-4.
- DIG-8 Team expansion proposal — CEO, blocked by DIG-4.

**Your next action:** answer the codebase list card on DIG-2. That is the only gate on the scan.

## Agent runs (6)

Every Paperclip heartbeat run recorded against this issue. A run's results are the comments, documents, attachments and commits it wrote (comment headers above name the run). Raw run transcripts are not archived, except the runs listed with a transcript copy below; see [inventory.md](../inventory.md#exclusions).

| Run | Agent | Status | Started | Finished | Log bytes | Transcript copy |
|---|---|---|---|---|---|---|
| `e4fb6029-6915-4174-98eb-c649ab510270` | CEO | failed | 2026-09-25T10:39:08.413Z | 2026-09-25T10:39:12.904Z | 2234 | — |
| `86152ab2-5c4b-4dca-bc80-54b16477f0e3` | CEO | failed | 2026-09-25T10:40:03.182Z | 2026-09-25T10:40:10.311Z | 2576 | — |
| `7b6affa5-3219-4d9c-9745-4438527e2419` | CEO | failed | 2026-09-25T10:41:02.564Z | 2026-09-25T10:41:05.966Z | 2576 | — |
| `5ce5ff19-fa8a-477b-929b-ce4f97518b16` | CEO | succeeded | 2026-09-25T11:05:29.575Z | 2026-09-25T11:06:05.622Z | 42187 | — |
| `dbef75a0-848b-4d0d-9d50-f141438e310a` | CEO | succeeded | 2026-09-25T11:13:24.066Z | 2026-09-25T11:15:07.488Z | 103123 | — |
| `49bb092e-91c3-496d-b89b-418f8156efc9` | CEO | succeeded | 2026-09-25T11:30:39.084Z | 2026-09-25T11:35:40.055Z | 300406 | — |
