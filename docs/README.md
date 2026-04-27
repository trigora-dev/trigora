# Trigora Docs

This documentation set is focused on the current Trigora alpha.

Use these guides to:

- get a project running locally
- understand flows, triggers, events, and context
- deploy hosted webhook flows
- manage deployed flows from the CLI

## Start Here

- [Getting Started](./getting-started.md)
- [Local Development](./local-development.md)
- [Hosted Deploys](./hosted-deploys.md)
- [Flow Management](./flow-management.md)

## Concepts

- [Flows](./concepts/flows.md)
- [Triggers](./concepts/triggers.md)
- [Events](./concepts/events.md)
- [Context](./concepts/context.md)

## Alpha Scope

Current alpha scope includes:

- local flow development with `trigger` and `dev`
- local webhook dev server for webhook flows
- hosted webhook deploys with `deploy`
- hosted flow listing, inspection, disable, and enable

Current limitations:

- hosted deploy currently supports webhook-triggered flows only
- webhook signature verification is not built in yet
- flow deletion is not available yet
- advanced hosted environment management is not documented or exposed yet
