# AI Agents

MCA Mail uses 7 specialised agents managed by the Orchestrator.

## Agent Summary

| Agent | Model Tier | When Used |
|---|---|---|
| Spam Agent | cheap | Every email |
| Classification Agent | cheap | Non-spam emails |
| Lead Qualification Agent | standard | Commercial emails |
| Logistics Expert Agent | capable | When expert advice needed |
| Company Research Agent | cheap | When enabled |
| Communication Agent | capable | Before handoff/draft |
| Handoff Agent | standard | When handoff triggered |

## Agent Interactions

- Not all agents run for every email
- Spam Agent runs first; spam/phishing emails exit early
- Classification runs next; non-commercial exits early
- Lead path: Qualification → (Research) → (Logistics) → Communication → (Handoff)